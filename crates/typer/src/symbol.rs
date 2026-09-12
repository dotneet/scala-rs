//! Symbols, scopes, and the compilation context.

use scala_rs_parser::{Flags, RefineDecl, SpecializedType, SpecializedTypes, SymbolId, Type};

/// Decl name that marks a refinement as the as-seen-from view of a type
/// projection rather than something the program wrote. Not a legal Scala
/// identifier, so it can never collide with a real member.
pub const AS_SEEN_FROM_MARK: &str = "<asSeenFrom>";

/// Second decl name of an as-seen-from view: the view is a *type*
/// projection `A#B` (a `#` whose prefix is a type, not a stable value) and
/// leaves an abstract type member of `B`'s enclosing class unsettled. Such a
/// member means a different type for every instance, so a member selected
/// through the projection takes it as nsc's existential `_1.T forSome { val
/// _1: A }` in parameter positions (`Typer::opaque_projection_params`).
pub const PROJECTION_MARK: &str = "<projection>";

thread_local! {
    /// Type parameters whose upper bound `is_sub_type` is already expanding.
    /// An F-bound (`A <: Rep[A]`) would otherwise recurse forever.
    static EXPANDING_BOUNDS: std::cell::RefCell<Vec<u32>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

thread_local! {
    /// Type-member aliases whose right-hand side is already being expanded.
    static EXPANDING_ALIASES: std::cell::RefCell<Vec<SymbolId>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

pub(crate) struct BoundGuard(bool);

impl Drop for BoundGuard {
    fn drop(&mut self) {
        if self.0 {
            EXPANDING_BOUNDS.with(|b| {
                b.borrow_mut().pop();
            });
        }
    }
}

struct AliasGuard;

impl Drop for AliasGuard {
    fn drop(&mut self) {
        EXPANDING_ALIASES.with(|b| {
            b.borrow_mut().pop();
        });
    }
}

thread_local! {
    /// Is the expansion in progress re-reading a *written* type from the
    /// class being typed, rather than reading a member through the prefix it
    /// was selected from? Then `self.T` must be left alone: the class doing
    /// the reading is not what a written prefix names. See
    /// `SymbolTable::expand_written_type`.
    static WRITTEN_TYPE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

struct WrittenGuard(bool);

impl Drop for WrittenGuard {
    fn drop(&mut self) {
        WRITTEN_TYPE.with(|c| c.set(self.0));
    }
}

/// Returns `None` when this alias's right-hand side is already being expanded.
/// `Type::TypeMember` carries no prefix, so an anonymous class that defines
/// `type R = (self.R, G.R)` while its parent declares an abstract `R` resolves
/// its own right-hand side back to itself, by name. nsc keeps the two apart by
/// the prefix; until we do, stop at the second visit instead of recursing until
/// the stack runs out (cats' `Representable#compose` is this shape).
fn enter_alias(id: SymbolId) -> Option<AliasGuard> {
    EXPANDING_ALIASES.with(|b| {
        let mut v = b.borrow_mut();
        if v.contains(&id) {
            note_walk_truncation();
            return None;
        }
        v.push(id);
        Some(AliasGuard)
    })
}

thread_local! {
    /// Type symbols one of the *chases* is already unfolding: the walks that
    /// replace an abstract type by what it stands for (`class_sym_of`,
    /// `widen_type_param`, `expand_applied_hk_alias`, and erasure's
    /// value-class unboxing, which enters through `enter_chase` too).
    ///
    /// nsc marks a symbol `LOCKED` while it completes it and raises
    /// `CyclicReference` when it is re-entered; `check::cyclic_type_defs`
    /// makes that same re-entry a diagnostic at the definition. These walks
    /// run *after* that check -- and on symbols that never went through it at
    /// all, because a pickle or a class file can carry a cycle we did not
    /// write -- so here re-entry is not raised, it simply answers "no more
    /// information" and lets the caller fall back to its own default.
    static CHASING: std::cell::RefCell<Vec<u64>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Which chase is asking. The walks are kept apart because they answer
/// different questions about the same symbol: `class_sym_of` looking through
/// `X` while erasure is unfolding `X` is not a cycle, and must not be told
/// that it is.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Chase {
    /// `SymbolTable::class_sym_of`.
    ClassOf = 1,
    /// `erasure::erase_ty`.
    Erase = 2,
    /// `SymbolTable::expand_applied_hk_alias`.
    HkAlias = 3,
}

/// Pops the symbol `enter_chase` pushed.
pub(crate) struct ChaseGuard;

impl Drop for ChaseGuard {
    fn drop(&mut self) {
        CHASING.with(|c| {
            c.borrow_mut().pop();
        });
    }
}

/// `None` when this chase is already unfolding `id`, which is exactly a cycle
/// in the type it stands for. Hold the guard for as long as the chase
/// recurses; dropping it re-opens the symbol.
pub(crate) fn enter_chase(kind: Chase, id: SymbolId) -> Option<ChaseGuard> {
    let key = ((kind as u64) << 32) | u64::from(id.0);
    CHASING.with(|c| {
        let mut v = c.borrow_mut();
        if v.contains(&key) {
            note_walk_truncation();
            return None;
        }
        v.push(key);
        Some(ChaseGuard)
    })
}

/// Returns `None` once the parent walk is implausibly deep, which only
/// happens when the hierarchy has a cycle.
fn enter_depth() -> Option<BoundGuard> {
    enter_parent_depth()
}

/// `enter_depth` for callers outside this module (`prefix.rs`).
pub(crate) fn enter_parent_depth() -> Option<BoundGuard> {
    EXPANDING_BOUNDS.with(|b| {
        let mut v = b.borrow_mut();
        if v.len() > 200 {
            note_walk_truncation();
            return None;
        }
        v.push(u32::MAX);
        Some(BoundGuard(true))
    })
}

thread_local! {
    /// Bookkeeping for the parent fan-outs in [`SymbolTable::is_sub_type`].
    /// See [`SymbolTable::walk_parents`] for what it is for.
    static SUBTYPE_WALK: std::cell::RefCell<SubtypeWalk> =
        const { std::cell::RefCell::new(SubtypeWalk::new()) };
}

/// Parent-walk steps one outermost question may take before the memo below is
/// engaged.
///
/// The memo costs two `Type` clones per distinct question, and the overwhelming
/// majority of questions are answered in a handful of steps by a hierarchy that
/// is a few nodes deep -- `String <: CharSequence` is three. Paying for a memo
/// there would slow down the hottest arm of the type checker to insure against
/// a shape it never meets. Above this many steps the walk has already proved it
/// is not that kind of question, and the memo is cheap next to what it saves.
///
/// Only a performance knob: engaging the memo later never changes an answer,
/// because nothing is ever *read* from the memo that was not first recorded by
/// this same walk.
const SUBTYPE_MEMO_AFTER: u32 = 256;

/// See [`SymbolTable::walk_parents`].
struct SubtypeWalk {
    /// How many parent walks are on the stack. The memo and the path describe
    /// one outermost question and are cleared when this returns to zero:
    /// `is_sub_type` takes `&self`, so no parent list can move under a walk,
    /// but they certainly move between one walk and the next.
    depth: u32,
    /// Parent-walk steps taken since the outermost walk began.
    steps: u32,
    /// The questions currently being answered, innermost last.
    path: Vec<(Type, Type)>,
    /// Questions already answered during this outermost question.
    memo: Vec<(Type, Type, bool)>,
    /// Bumped whenever a question is answered `false` only because it was
    /// already on `path`. A result is memoisable only while this stands still
    /// across it.
    truncations: u32,
}

impl SubtypeWalk {
    const fn new() -> Self {
        SubtypeWalk {
            depth: 0,
            steps: 0,
            path: Vec::new(),
            memo: Vec::new(),
            truncations: 0,
        }
    }
}

/// Record that an ambient guard refused an expansion.
///
/// `enter_bound`, `enter_alias`, `enter_chase` and `enter_depth` all answer
/// "stop" based on what is *already* being expanded, so a subtype question
/// asked underneath one of them can get a different answer than the same
/// question asked on its own -- an F-bound (`A <: Rep[A]`) is not re-expanded
/// while it is already being expanded, and the answer may be `false` only for
/// that reason. Memoising such a result would leak one caller's ambient state
/// into another's answer, which is the same hazard as memoising a result that
/// truncated at `path`, so it is counted the same way and suppresses the same
/// memo entries.
fn note_walk_truncation() {
    SUBTYPE_WALK.with(|w| {
        let mut w = w.borrow_mut();
        if w.depth > 0 {
            w.truncations += 1;
        }
    });
}

/// Pops one parent walk off [`SUBTYPE_WALK`], and empties it once the
/// outermost one returns.
struct WalkGuard {
    /// Whether this walk pushed its question onto `path`.
    tracked: bool,
}

impl Drop for WalkGuard {
    fn drop(&mut self) {
        SUBTYPE_WALK.with(|w| {
            let mut w = w.borrow_mut();
            if self.tracked {
                w.path.pop();
            }
            w.depth -= 1;
            if w.depth == 0 {
                w.steps = 0;
                w.truncations = 0;
                w.path.clear();
                w.memo.clear();
            }
        });
    }
}

/// Returns `None` when this parameter's bound is already being expanded.
fn enter_bound(id: SymbolId) -> Option<BoundGuard> {
    EXPANDING_BOUNDS.with(|b| {
        let mut v = b.borrow_mut();
        if v.contains(&id.0) {
            note_walk_truncation();
            return None;
        }
        v.push(id.0);
        Some(BoundGuard(true))
    })
}

use rustc_hash::FxHashMap as HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymKind {
    NoSymbol,
    Package,
    Class,
    Module,
    ModuleClass,
    Method,
    Term,
    TypeParam,
    TypeMember,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Intrinsic {
    None,
    Println,
    Print,
    IntBin(&'static str),
    IntUn(&'static str),
    LongBin(&'static str),
    LongUn(&'static str),
    DoubleBin(&'static str),
    DoubleUn(&'static str),
    FloatBin(&'static str),
    FloatUn(&'static str),
    BoolBin(&'static str),
    BoolUn(&'static str),
    StringConcat,
    AnyToString,
    Identity,
    IntToLong,
    IntToDouble,
    IntToByte,
    IntToShort,
    LongToDouble,
    Assert,
    Require,
    NotImplemented,
    StringToInt,
    StringToLong,
    StringToDouble,
    WrapArrowAssoc,
    Locally,
    Any2StringAdd,
    Implicitly,
    /// AnyRef reference equality (`eq`).
    Eq,
    /// AnyRef reference inequality (`ne`).
    Ne,
    /// Universal equality (`Any.==`).
    AnyEq,
    /// Universal inequality (`Any.!=`).
    AnyNe,
    /// `Any.synchronized` (monitor enter/exit around a by-name body).
    Synchronized,
    /// `x.isInstanceOf[T]` / `x.asInstanceOf[T]`: `instanceof` and a
    /// checkcast-or-unbox against the type argument.
    IsInstanceOf,
    AsInstanceOf,
    /// Numeric widening the JVM needs an instruction for.
    IntToFloat,
    LongToFloat,
    FloatToDouble,
    /// `x.##`: `Statics.anyHash`, or the primitive-specific hash.
    AnyHash,
    /// `"%d".format(args)`: `java.lang.String.format`.
    StringFormat,
    /// `classOf[T]`: a class constant for the erasure of `T`.
    ClassOf,
    /// `Any.getClass`: `Object.getClass`, or the boxed `TYPE` for a primitive.
    GetClass,
    /// `TupleN.apply` — nsc allocates the tuple directly, so no `TupleN$`
    /// module classfile is needed on the private runtime.
    NewTuple(usize),
    /// `Predef.int2Integer` and its seven siblings: box a primitive into its
    /// `java.lang` wrapper. The payload is the JVM descriptor letter of the
    /// primitive (`I`, `J`, `Z`, ...), so the emitter picks the right
    /// `valueOf` even when the argument arrived as a narrower type
    /// (`java.lang.Integer = 'c'` passes a `Char`).
    BoxValue(&'static str),
    /// `Predef.Integer2int` and siblings: the reverse, `Integer.intValue`.
    UnboxValue(&'static str),
    /// One of the 49 `toByte`/`toShort`/`toChar`/`toInt`/`toLong`/`toFloat`/
    /// `toDouble` members nsc declares on every numeric value class. The
    /// payload is `<from><to>` in JVM descriptor letters (`"IB"` is
    /// `Int.toByte`), because the receiver's *static* type is what picks the
    /// instruction sequence and `tree.ty` only carries the target.
    NumConv(&'static str),
    /// `scala.concurrent.duration.package$.DurationInt` and its `DurationLong`
    /// / `DurationDouble` siblings: an implicit conversion whose target is a
    /// value class, so nsc lowers it to `new <Box>(arg)` rather than to a call.
    ///
    /// The conversion itself really does exist on the package object, but it
    /// is erased to the identity on the underlying primitive
    /// (`DurationInt(int)int`), and the unit methods (`seconds`, `millis`, …)
    /// are ordinary instance methods of the *boxed* `package$DurationInt`.
    /// Emitting the call and then selecting on its `int` result is what
    /// `javap` of scalac's own output rules out: it writes
    /// `new package$DurationInt(5)` and calls `seconds()` on that.
    ///
    /// The box class is the conversion's declared result type and the
    /// constructor's argument its declared parameter type.
    NewWrapper,
}

/// What a `def f = macro Impl.method` binds to.
///
/// nsc stores the equivalent as a pickled `@macroImpl(tree)` annotation on the
/// macro def symbol so that a *separately compiled* macro def can still be
/// expanded. We keep the same three facts, in the form the expander needs:
/// the JVM class that holds the implementation, the method name on it, and
/// whether the def was declared with a `blackbox` or `whitebox` context.
/// One type argument written on a macro implementation *reference*, resolved
/// as far as it can be at the point the binding is made.
///
/// `def mapTo[R] = macro ShapedValue.mapToImpl[R, U]` writes two: `R`, which
/// is `mapTo`'s own type parameter, and `U`, which is `ShapedValue`'s. nsc
/// keeps the pair as `MacroImplBinding.targs` and resolves each one at the
/// call site (`Macros.macroArgs`) -- it never lines the implementation's tags
/// up with the call site's type arguments, which is why `mapTo[MyRow]` can
/// supply one type argument to an implementation that asks for two tags.
///
/// The resolution is deliberately split in two: which *kind* of thing the
/// reference names is settled once, where the macro def is bound and both its
/// own type parameters and its owner's are in scope; what it stands for is
/// settled at each call site, where the call's type arguments and its prefix
/// are.
#[derive(Clone, Debug, PartialEq)]
pub enum MacroTarg {
    /// A type parameter of the macro **def**, at this position in its own type
    /// parameter list: the call site's type argument at the same position.
    /// nsc matches it by name (`macroDef.typeParams.indexWhere(_.name ==
    /// targ.name)`), and so does the code that builds this.
    DefParam { index: usize, name: String },
    /// A type parameter of the macro def's **owner**, at this position in the
    /// owner's type parameter list: `asSeenFrom` the call's prefix. `owner` is
    /// the class the parameter belongs to, which is the base class the
    /// receiver's type has to be seen as.
    OwnerParam {
        owner: SymbolId,
        index: usize,
        name: String,
    },
    /// A type written out in full and carrying no type arguments of its own
    /// (`macro Impl.f[R, Int]`). nsc uses it as it stands.
    Fixed(Type),
    /// A type argument scala-rs will not resolve, carrying its spelling for
    /// the diagnostic. This is not the same as "nsc cannot either": nsc reads
    /// `binding.targs(i).tpe.typeSymbol` and then that *symbol's* own type, so
    /// `macro Impl.f[List[R], U]` gives the implementation the raw `List[A]` --
    /// `A` being `List`'s own type parameter, not anything at the call site.
    /// Real scalac 2.13.16 prints exactly that, and
    /// `tests/fixtures/mt2_bad.scala` is the program that shows it. Copying
    /// that would hand an implementation a type with a free parameter in it;
    /// substituting instead would hand it `List[String]`, which is not what
    /// nsc says. So it is refused by name.
    Unresolved(String),
}

/// Source binding payload retained before uncurry for nsc's macroImpl annotation.
#[derive(Clone, Debug, PartialEq)]
pub struct MacroPickle {
    pub signature: Vec<Vec<i32>>,
    pub targs: Vec<Type>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MacroBinding {
    pub pickle: Option<MacroPickle>,
    /// JVM internal name of the class holding the implementation, e.g. `M$`.
    /// nsc requires the implementation to be a method of an object, so this is
    /// always a module class.
    pub impl_class: String,
    /// Method name on `impl_class`.
    pub impl_method: String,
    /// `false` for `scala.reflect.macros.whitebox.Context`. Blackbox macros keep
    /// the declared result type; whitebox macros may refine it, which changes
    /// how the call site re-typechecks the expansion.
    pub blackbox: bool,
    /// How many `c.WeakTypeTag[T]` arguments the implementation's trailing
    /// implicit clause takes. nsc records the same thing per parameter as a
    /// `Tagged(i)` fingerprint; the expander needs it because the tags are
    /// *optional* -- an implementation may declare none -- and sending the
    /// wrong number is an `IllegalArgumentException` inside the engine
    /// rather than a diagnostic.
    pub tag_params: usize,
    /// For each value parameter of the implementation, in order and after the
    /// leading `Context`: `true` when it is declared `c.Expr[T]`, `false` when
    /// it is the raw `c.Tree` nsc has also allowed since 2.11 (slick's
    /// `mapToImpl` takes `Tree`s). Read off the *source* signature, because a
    /// class file scala-rs writes erases both to `Object`.
    pub expr_args: Vec<bool>,
    /// What each of the `tag_params` tags stands for, in the order the
    /// implementation's trailing clause asks for them: the type argument
    /// written on the implementation reference that nsc's fingerprint for that
    /// parameter points at.
    ///
    /// **Empty means "not known"**, not "no tags". A binding whose reference
    /// could not be read this way falls back to the older rule -- line the tags
    /// up one for one with the call site's type arguments -- which is right
    /// whenever the reference is the usual `macro Impl.f[A]` and is refused
    /// with a reason when it is not. When this is non-empty it has exactly
    /// `tag_params` entries.
    pub tag_targs: Vec<MacroTarg>,
}

impl MacroBinding {
    /// `Class.method`, the pair that identifies the implementation. Two
    /// symbols with the same origin stand for the same macro def.
    pub fn origin(&self) -> String {
        format!("{}.{}", self.impl_class, self.impl_method)
    }
}

#[derive(Clone, Debug)]
pub struct Symbol {
    pub id: SymbolId,
    pub name: String,
    pub owner: SymbolId,
    pub kind: SymKind,
    pub flags: Flags,
    pub ty: Type,
    pub members: Vec<SymbolId>,
    pub jvm_name: String,
    /// Verified hidden enclosing-instance descriptor from a binary constructor.
    pub binary_outer_desc: Option<String>,
    pub intrinsic: Intrinsic,
    /// Constructor / method parameter symbols (flat, first clause).
    pub params: Vec<SymbolId>,
    pub paramss: Vec<Vec<SymbolId>>,
    /// The number of parameters in each clause the source actually wrote,
    /// recorded before `uncurry` joins the clauses into one.
    ///
    /// nsc's pickler runs *before* `uncurry`, so a class file carries
    /// `def f()(implicit e: E): T` as nested `MethodType`s. Ours flattens the
    /// clauses off the symbol long before the backend sees it, and a pickle
    /// written from the flattened shape tells a reader -- scalac included --
    /// that the method is `def f(e: E): T`, which rejects the call site that
    /// writes `f()`. Empty when the method still carries its own clauses
    /// (`Symbol::ty`), which is every method `uncurry` left alone.
    pub pickle_clauses: Vec<usize>,
    /// Known Scala declaration shape: true for `def f: T`, false for a
    /// declaration with value clauses. None for descriptor-only/prelude data.
    pub parameterless_method: Option<bool>,
    /// For case classes / classes: constructor parameter field names.
    pub ctor_fields: Vec<SymbolId>,
    pub parents: Vec<Type>,
    pub default_rhs: Option<scala_rs_parser::Tree>,
    /// Class or method type parameters, in order.
    pub tparams: Vec<SymbolId>,
    /// Direct subclasses / objects of a sealed parent (same compilation unit).
    pub children: Vec<SymbolId>,
    /// Self type (`trait T { self: Foo => }`).
    pub self_type: Option<Type>,
    /// The self *alias* a template introduces (`self` in `trait T { self: Foo => }`).
    /// It is scoped to the template that writes it: unlike an ordinary member
    /// it is not inherited, so every place that copies another template's
    /// members into scope has to leave it out — otherwise two components of a
    /// cake that both call their alias `self` collide into an overload.
    pub self_alias: Option<SymbolId>,
    /// Access qualifier `private[C]` / `protected[C]` (`C` is a class or package name).
    /// `private[this]` is `PRIVATE|LOCAL` with this field empty.
    pub private_within: Option<String>,
    /// A `private` member the companion reads. The JVM has no companions, so
    /// nsc widens such a member (`Counter$$step`); we drop `ACC_PRIVATE`.
    pub access_widened: bool,
    /// nsc `scala.LowPriorityImplicits`: `Predef` inherits `intWrapper` &
    /// friends from a superclass, so a conversion declared in `Predef` itself
    /// (`double2Double`) wins the tie for a member both results offer.
    /// `0.5.isNaN` really is `Predef.double2Double(0.5).isNaN()` in scalac, not
    /// `RichDouble.isNaN`. Modelling the superclass would change the emitted
    /// owner of every wrapper call, so the priority is recorded here instead.
    pub low_priority: bool,
    /// A Java field declared as Object accepts boxing on writes, while its
    /// reads have the reference type AnyRef. Generic T fields do not widen.
    pub java_object_field: bool,
    /// Language annotations (`@deprecated(...)`, `@tailrec`, …) copied from modifiers.
    pub annotations: Vec<scala_rs_parser::Tree>,
    /// Lower bound of an abstract/HK type member (`type F[_] >: Lo`).
    pub bound_lo: Option<Type>,
    /// Upper bound of an abstract/HK type member (`type F[_] <: Hi`).
    pub bound_hi: Option<Type>,
    /// Whether this type-member symbol came from a declaration with a
    /// right-hand side (`type T = U`).  The RHS may itself be another
    /// `TypeMember`; the pickle writer must retain that declaration kind
    /// instead of inferring it from the resolved type alone.
    pub is_type_alias: bool,
    /// A rigid existential type introduced by a pattern, never an inference variable.
    pub is_pattern_skolem: bool,
    /// For classes defined inside a method: enclosing-method locals the class
    /// reads. Each becomes a private field plus a trailing constructor
    /// parameter (see `anon_capture`).
    pub captures: Vec<SymbolId>,
    /// Set on `def f = macro Impl.method`. Such a symbol has no bytecode: every
    /// call site must be replaced by the implementation's expansion.
    pub macro_impl: Option<MacroBinding>,
    /// JVM internal name of the class that actually *declares* this method,
    /// when the owner's own class file does not reach it.
    ///
    /// A member completed from a library pickle is installed on the class it
    /// was asked for, because that is where the typer has to find it. The JVM
    /// method it compiles to may be declared somewhere the bytecode hierarchy
    /// does not lead: `scala.reflect.api.JavaUniverse` is an interface with
    /// `interfaces: 0`, and `Constant()` is declared on
    /// `scala.reflect.api.Constants`, reachable only through the abstract
    /// class `Universe` that the class file cannot name. Naming the queried
    /// class in the call is then a `NoSuchMethodError` at the first
    /// invocation, so codegen names this class instead and `checkcast`s the
    /// receiver to it -- exactly what nsc emits.
    ///
    /// Empty when the owner's own class file reaches the declaration, which is
    /// every ordinary member.
    pub declaring_class: String,
    /// Whether [`Symbol::declaring_class`] is an interface, and so whether the
    /// call is `invokeinterface` or `invokevirtual`. Meaningless when
    /// `declaring_class` is empty.
    pub declaring_is_interface: bool,
    /// The pickled *declaration* a member completed from a library pickle
    /// stands for: `<declaring class>#<jvm name><erased parameter descriptors>`.
    ///
    /// `PickleSupply` installs an inherited member on the class it was asked
    /// for, so the one `IterableOps.map` is copied onto `immutable.Seq` when a
    /// `Seq` receiver asks for it and onto `collection.IndexedSeq` when an
    /// `IndexedSeq` receiver does. `immutable.IndexedSeq` has both above it and
    /// then sees two `map`s, differing only in the vocabulary each copy was
    /// rewritten into -- one declaration, not an overload set. This is what
    /// says so; `Typer::collapse_pickled_copies` (check.rs) collapses them.
    ///
    /// Empty for everything the prelude, a source file or a class file
    /// declares.
    pub pickled_origin: String,
    /// Ancestors of the original pickle declaration's owner. Completing a
    /// member on a receiver must not erase the owner's specificity relation.
    pub pickled_owner_bases: Vec<String>,
    /// nsc `ABSOVERRIDE`: the source wrote `abstract override`, so `super` in
    /// this member is bound by the *linearization* of whatever concrete class
    /// mixes the trait in. `flags` cannot carry this: the namer already sets
    /// `ABSTRACT` on every body-less `def`, so `override def close(): Unit`
    /// (deferred) and `abstract override def close(): Unit = …` (stackable)
    /// are indistinguishable there.
    pub abstract_override: bool,
    /// nsc `SUPERACCESSOR`: this trait member's body writes `super.m`, so the
    /// trait declares a `p$q$T$$super$m` accessor that every class mixing it
    /// in has to implement (nsc's mixin phase does that only when it finds
    /// the flag in the trait's *signature*, which is why it is pickled).
    pub super_accessor: bool,
    /// `@specialized` on a **type parameter**: the types it selects, read the
    /// way nsc's `specializedOn` reads them (see
    /// `scala_rs_parser::specialization`). `None` when the parameter carries
    /// no `@specialized`, and `Some(empty)` when it carries one that names
    /// nothing specializable.
    ///
    /// Recording it is the typer half of specialization. The post-pickler
    /// method slice consumes one method-owned parameter and emits Int/Long
    /// entries; class and trait entries remain for a later phase.
    pub specialized: Option<scala_rs_parser::SpecializedTypes>,
    /// `@unspecialized` on a member: nsc's opt-out from the specialization its
    /// owner would otherwise give it. Recorded for the same reason, and with
    /// the same nothing reading it yet.
    pub unspecialized: bool,
    /// nsc `DEFERRED` for a **value**: `val v: Int` / `var v: Int` written with
    /// no right-hand side. The namer sets `ABSTRACT` on a body-less `def` but
    /// not on a body-less `val`, so without this an abstract `val` in a trait
    /// looks exactly like a concrete one and `class C extends T` cannot tell
    /// whether `v` still needs implementing.
    pub deferred_val: bool,
    /// nsc `DEFERRED` for a **method** that this run did not parse: one
    /// supplied from a library pickle, or one the prelude declares by hand.
    ///
    /// A `def` with no body in this run's own sources carries
    /// `Flags::ABSTRACT` (the namer sets it), and so does one read by the
    /// eager `-cp` classfile scan (`classpath.rs` copies the pickle's
    /// `DEFERRED`). Neither the prelude nor `PickleSupply` can say it that
    /// way: `prelude::method` stamps every member `Flags::FINAL`, and
    /// `PickleSupply` allocates members `Flags::EMPTY` on purpose -- see
    /// `override_check::modifiers_are_known`, which withholds every
    /// modifier-shaped diagnostic for exactly those two groups. Widening
    /// `Flags::ABSTRACT` to cover them would turn those diagnostics on for a
    /// whole library at once; this bit records the fact without doing that.
    ///
    /// Read through [`SymbolTable::method_is_deferred`].
    pub deferred_method: bool,
    /// This `val` / `var` lives in a **separately compiled** class whose
    /// backing field scalac made `private`, so every read goes through the
    /// public accessor of the same name and every write through `name_$eq`.
    ///
    /// `javap -p` on scalac's `class Holder(val n: Int)`:
    ///
    /// ```text
    /// private final int n;
    /// public int n();
    /// ```
    ///
    /// Reading such a member with `getfield` passes the JVM verifier -- field
    /// access control is checked at *resolution*, not at verification -- and
    /// throws `IllegalAccessError` the first time the method runs. This is
    /// invisible when scala-rs compiles both sides, because scala-rs emits the
    /// field itself public; it only appears across compilers.
    ///
    /// Set by `classpath::install_classpath` from the class file, which is the
    /// ground truth about which members actually have an accessor: a
    /// `private[this] val` has none and keeps the direct field read.
    pub via_accessor: bool,
    /// The **block** this class or object was declared directly in, when it
    /// was declared in one: `(file index, the block's `NodeId`)`, packed.
    /// `None` for everything that is a member of a template or a package.
    ///
    /// A companion pair has to be *co-defined*, and for a local definition
    /// nsc spells that as the same `Scope`, not the same owner
    /// (`Contexts.lookupSibling`, whose comment is this exact program):
    ///
    /// ```text
    /// // Must be owned by the same Scope, to ensure that in
    /// // `{ class C; { ...; object C } }`, the class is not seen as a
    /// // companion of the object.
    /// ```
    ///
    /// Two local definitions in *different* blocks of one method share an
    /// owner, so the owner cannot tell them apart, and
    /// `test/files/neg/t8002-nested-scope.scala` is the program where that
    /// matters: the inner `object C` is not the companion of the inner
    /// `class C` and may not read its `private def x`.
    pub local_scope: Option<u64>,
}

/// One method-owned specialization entry created after pickling.
///
/// The generic method remains the source-level declaration and the source
/// pickle.  This record names the additional JVM entry and the primitive that
/// its cloned body uses; synthetic entries are deliberately created after the
/// pickle snapshot so they cannot become Scala declarations accidentally.
#[derive(Clone, Debug)]
pub struct MethodVariant {
    pub original: SymbolId,
    pub symbol: SymbolId,
    pub type_param: SymbolId,
    pub selected: SpecializedType,
    pub ty: Type,
    pub jvm_name: String,
}

impl Symbol {
    pub fn is_class_like(&self) -> bool {
        matches!(self.kind, SymKind::Class | SymKind::ModuleClass)
    }
}

/// Which prelude symbol a source definition may take the place of. Scala
/// keeps terms and types in separate namespaces, so a source `object Option`
/// replaces the prelude's module and leaves its class alone, and a source
/// `trait Option` does the opposite. A source class also replaces a prelude
/// *alias* of that name (`type Seq[+A] = …` in the `scala` package object is
/// what `Seq.scala` defines).
/// The term half of Scala's two namespaces. `SymKind::ModuleClass` is not in
/// it: a module class answers to `X$`, never to `X`.
fn is_term_kind(k: SymKind) -> bool {
    matches!(k, SymKind::Module | SymKind::Method | SymKind::Term)
}

/// The type half. `SymKind::ModuleClass` is left out for the same reason.
fn is_type_kind(k: SymKind) -> bool {
    matches!(k, SymKind::Class | SymKind::TypeMember)
}

fn shadowable_kind(new: SymKind, old: SymKind) -> bool {
    match new {
        SymKind::Class => matches!(old, SymKind::Class | SymKind::TypeMember),
        SymKind::Module => old == SymKind::Module,
        SymKind::ModuleClass => old == SymKind::ModuleClass,
        _ => false,
    }
}

/// SLS 2 ("Identifiers, Names and Scopes") ranks the ways a simple name can
/// be bound, and a binding of higher precedence **hides** one of lower
/// precedence in the same scope -- it does not merely join it. Lower is
/// stronger.
///
/// Without this, every route into a scope entered its symbol the same way and
/// the answer was decided by insertion order and symbol id. gitbucket writes
///
/// ```text
/// import gitbucket.core.model.Profile.profile.blockingApi._
/// import gitbucket.core.servlet.Database
/// ```
///
/// with a comment saying why the second line has to be there, and we bound
/// `Database` to slick's -- a *wrong program*, not a diagnostic: the eleven-line
/// reduction in `docs/gitbucket.md` compiles either way and prints the other
/// object's answer.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default, Hash)]
pub enum BindRank {
    /// (1) A definition or declaration that is local, inherited, or made
    /// available by a package clause **in the same compilation unit** as the
    /// reference.
    #[default]
    Definition,
    /// (2) An explicit import (`import p.X`, `import p.{X => Y}`).
    Explicit,
    /// (3) A wildcard import (`import p._`), including the `scala._` and
    /// `java.lang._` every source carries.
    Wildcard,
    /// (4) A definition made available by a package clause, but defined in
    /// **another** compilation unit. This one ranks *below* a wildcard import,
    /// which is why the level cannot be collapsed into `Definition`.
    PackageElsewhere,
}

/// One binding of a simple name in one scope.
#[derive(Clone, Copy, Debug)]
pub struct Binding {
    pub sym: SymbolId,
    pub rank: BindRank,
    /// Which `import` clause put it here, or 0 for anything that is not an
    /// import. Two bindings of one name at the same precedence are an
    /// *ambiguous reference* when they come from two different import clauses
    /// (SLS 2; nsc `Contexts.ambiguousImports`), and an ordinary overload set
    /// when they come from the same one -- `import p._` over an object with
    /// two `def f` is one import, however many of its ancestors declare them.
    pub origin: u64,
}

#[derive(Clone, Debug, Default)]
pub struct Scope {
    /// Template whose members and body imports this scope exposes.
    pub(crate) template_owner: Option<SymbolId>,
    map: HashMap<String, Vec<Binding>>,
    /// Owners brought in by a wildcard import (`import p._`) in this scope,
    /// with the names that selector hid (`import p.{X => _, _}`).
    /// A package read from a jar cannot be enumerated up front, so the names
    /// it offers are resolved on demand: see `Checker::expose_unqualified`.
    wildcards: Vec<WildcardImport>,
}

/// `import owner._`, minus the names hidden by `X => _` selectors.
#[derive(Clone, Debug)]
pub struct WildcardImport {
    pub origin: u64,
    pub owner: SymbolId,
    pub hidden: Vec<String>,
}

impl WildcardImport {
    pub fn offers(&self, name: &str) -> bool {
        !self.hidden.iter().any(|h| h == name)
    }
}

impl Scope {
    pub fn enter(&mut self, name: &str, id: SymbolId) {
        self.enter_ranked(name, id, BindRank::Definition);
    }

    /// Enter `id` under `name` at SLS 2 precedence `rank`.
    pub fn enter_ranked(&mut self, name: &str, id: SymbolId, rank: BindRank) {
        self.enter_binding(name, id, rank, 0);
    }

    /// [`Self::enter_ranked`] for a binding an `import` clause makes, tagged
    /// with which clause it was.
    pub fn enter_binding(&mut self, name: &str, id: SymbolId, rank: BindRank, origin: u64) {
        let slot = self.map.entry(name.to_string()).or_default();
        // One symbol reachable by two routes is still one symbol, not an
        // overload: a template's self alias, for instance, is entered both
        // with the rest of the class's members and by `bind_self_type`.
        // Reached twice at two precedences it binds at the better one -- an
        // inherited member a wildcard import also offers is still an
        // inherited member, and two imports naming the same symbol are not
        // ambiguous (SLS 2), so the better rank keeps its origin.
        if let Some(e) = slot.iter_mut().find(|b| b.sym == id) {
            if rank < e.rank {
                e.rank = rank;
                e.origin = origin;
            }
            return;
        }
        slot.push(Binding {
            sym: id,
            rank,
            origin,
        });
    }

    /// Put `with` where `victims` stood, under `name`.
    ///
    /// Used when a source definition replaces a prelude symbol: the prelude
    /// entered `IterableOnce` into the base scope by hand, and that entry has
    /// to become the source trait rather than merely be joined by it.
    /// Returns whether anything was there.
    pub fn replace(&mut self, name: &str, victims: &[SymbolId], with: SymbolId) -> bool {
        let Some(slot) = self.map.get_mut(name) else {
            return false;
        };
        if !slot.iter().any(|b| victims.contains(&b.sym)) {
            return false;
        }
        let rank = slot
            .iter()
            .filter(|b| victims.contains(&b.sym))
            .map(|b| b.rank)
            .min()
            .unwrap_or_default();
        slot.retain(|b| !victims.contains(&b.sym));
        if !slot.iter().any(|b| b.sym == with) {
            slot.push(Binding {
                sym: with,
                rank,
                origin: 0,
            });
        }
        true
    }

    pub fn enter_wildcard(&mut self, owner: SymbolId, hidden: &[String]) {
        self.enter_wildcard_origin(owner, hidden, 0);
    }

    pub fn enter_wildcard_origin(&mut self, owner: SymbolId, hidden: &[String], origin: u64) {
        if let Some(w) = self
            .wildcards
            .iter_mut()
            .find(|w| w.owner == owner && w.origin == origin)
        {
            w.hidden.retain(|h| hidden.iter().any(|n| n == h));
            return;
        }
        self.wildcards.push(WildcardImport {
            origin,
            owner,
            hidden: hidden.to_vec(),
        });
    }

    pub fn wildcards(&self) -> &[WildcardImport] {
        &self.wildcards
    }

    /// Every binding of `name` in this scope, with its precedence.
    pub fn lookup_ranked(&self, name: &str) -> &[Binding] {
        self.map.get(name).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// The bindings of `name` a reference in this scope can see: the ones at
    /// the best precedence present, since a stronger binding *hides* a weaker
    /// one rather than joining it as an overload (SLS 2).
    pub fn lookup(&self, name: &str) -> Vec<SymbolId> {
        best_ranked(self.lookup_ranked(name), |_| true)
    }

    pub fn names(&self) -> impl Iterator<Item = &String> {
        self.map.keys()
    }

    /// How many names this scope binds.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Every binding, in the same order `names` yields. `implicits_in_scope`
    /// walks every name of every enclosing scope on every implicit search;
    /// going through `names` and then `lookup` hashed each name twice.
    pub fn entries(&self) -> impl Iterator<Item = (&String, &[Binding])> {
        self.map.iter().map(|(k, v)| (k, v.as_slice()))
    }
}

/// The entries of one scope slot that `pred` accepts, kept down to the best
/// SLS 2 precedence among them. The namespace filter runs *first*: the two
/// namespaces are ranked separately, so a `type T` a wildcard import offers is
/// not hidden by an explicitly imported *value* `T`.
fn best_ranked(slot: &[Binding], pred: impl Fn(SymbolId) -> bool) -> Vec<SymbolId> {
    let best = slot.iter().filter(|b| pred(b.sym)).map(|b| b.rank).min();
    let Some(best) = best else { return Vec::new() };
    slot.iter()
        .filter(|b| b.rank == best && pred(b.sym))
        .map(|b| b.sym)
        .collect()
}

/// Whether the surviving bindings of one slot come from two different
/// `import` clauses -- SLS 2's ambiguous reference, which nsc reports as
/// "reference to X is ambiguous". Two clauses naming the *same* symbol are
/// not ambiguous, and neither is an overload set one clause brings in.
fn two_imports_tie(slot: &[Binding], pred: impl Fn(SymbolId) -> bool) -> bool {
    let Some(best) = slot.iter().filter(|b| pred(b.sym)).map(|b| b.rank).min() else {
        return false;
    };
    let mut origin: Option<u64> = None;
    for b in slot.iter().filter(|b| b.rank == best && pred(b.sym)) {
        if b.origin == 0 {
            continue;
        }
        match origin {
            None => origin = Some(b.origin),
            Some(o) if o != b.origin => return true,
            _ => {}
        }
    }
    false
}

pub struct SymbolTable {
    /// Stable module outer arguments resolved from a parent's singleton path.
    pub parent_outer_modules: HashMap<SymbolId, SymbolId>,
    /// Original RHS prefixes of binary aliases, keyed by declaring owner/name.
    /// These are declaration metadata, not a cache of call-site receivers.
    pub binary_alias_prefixes: HashMap<(SymbolId, String), scala_rs_pickle::sym::SigType>,
    /// Classes whose class file has been read (`classpath::apply_java_class_meta`):
    /// only for these does the absence of `Flags::STATIC` say a nested class
    /// is not static (`prefix.rs`, `is_binary_nested_class`). A stub knows
    /// nothing yet.
    pub binary_read: rustc_hash::FxHashSet<u32>,
    /// Deferred names a library class was *proven* to override, keyed by the
    /// class symbol.
    ///
    /// `PickleSupply` installs members on demand, so a library trait's symbol
    /// carries only what something asked for, and an override nobody asked for
    /// reads as absent. `check_infer::sam_sig_here` repairs that for the one
    /// question it matters to -- how many abstract methods a SAM candidate has
    /// -- by reading the pickle (`PickleSupply::concrete_method_names`) rather
    /// than installing the members, because completion is additive global
    /// state. But the *answer* has to outlive that one call: erasure
    /// (`erasure.rs`) and the backend (`gen_lambda.rs`) ask
    /// [`Self::sam_sig`] again, with no pickle to reach for, and a `None`
    /// there emits a plain `scala.Function2` for a tree the typer had already
    /// adapted to the SAM type -- `cats.kernel.Order.toOrdering` returned a
    /// `Function2` from a method typed `Ordering[A]`, and its unboxed `Int`
    /// result made the whole class fail to verify. So the proof is recorded
    /// here, where every later `sam_sig` sees it. Only SAM counting reads it;
    /// nothing installs a member on the strength of it.
    pub sam_known_overrides: rustc_hash::FxHashMap<u32, Vec<String>>,
    pub symbols: Vec<Symbol>,
    pub scopes: Vec<Scope>,
    pub root: SymbolId,
    pub scala_pkg: SymbolId,
    pub predef: SymbolId,
    pub singleton_sym: SymbolId,
    pub any_sym: SymbolId,
    pub anyref_sym: SymbolId,
    pub anyval_sym: SymbolId,
    pub int_sym: SymbolId,
    pub byte_sym: SymbolId,
    pub short_sym: SymbolId,
    pub char_sym: SymbolId,
    pub long_sym: SymbolId,
    pub float_sym: SymbolId,
    pub double_sym: SymbolId,
    pub boolean_sym: SymbolId,
    pub unit_sym: SymbolId,
    pub string_sym: SymbolId,
    pub array_sym: SymbolId,
    pub option_sym: SymbolId,
    pub some_sym: SymbolId,
    pub none_sym: SymbolId,
    pub list_sym: SymbolId,
    pub nil_sym: SymbolId,
    pub cons_sym: SymbolId,
    pub object_sym: SymbolId,
    /// Enclosing owner while naming/typing.
    pub owner: SymbolId,
    pub this_class: SymbolId,
    /// Source-trait `super` targets that have no declaration in that trait.
    /// The backend pickles these as `SUPERACCESSOR` aliases so scalac's mixin
    /// phase can synthesize the forwarding method in an external subclass.
    pub super_accessor_targets: rustc_hash::FxHashMap<SymbolId, Vec<(SymbolId, Vec<Type>)>>,
    /// Source method identities that were proven to be overrides before
    /// erasure.  Backend dispatch must not rediscover this relation from the
    /// erased JVM types: a generic base and an unrelated overload can erase
    /// to the same descriptor.
    /// Class-specific inherited implementations proven before erasure.
    pub inherited_method_implementations: rustc_hash::FxHashSet<(SymbolId, SymbolId, SymbolId)>,
    pub method_override_families: rustc_hash::FxHashSet<(SymbolId, SymbolId)>,
    /// The complement, and equally pre-erasure: source method pairs *proven*
    /// to be two methods -- an overload no erasure can reunite. The backend
    /// needs this to tell a bridge from an overload, because
    /// `bridge_overrides` compares erased descriptors on purpose and by then
    /// `Ops.pp[B](xs: Bag[B])` and `Table.pp[V2](xs: Bag[(K, V2)])` are the
    /// same `(LBag;)` parameter. Only membership is meaningful: absence means
    /// "not proven", never "these override".
    pub method_overload_pairs: rustc_hash::FxHashSet<(SymbolId, SymbolId)>,
    /// Terms whose pre-erasure type was a user value class, and which one.
    /// Erasure replaces the type with the underlying representation, but the
    /// backend still has to know that `case class Box(m: Meters)` prints its
    /// field as a boxed `Meters`.
    pub value_class_terms: rustc_hash::FxHashMap<SymbolId, SymbolId>,
    /// Source names of constructor arguments whose storage was expanded.
    pub constructor_parameter_names: rustc_hash::FxHashMap<SymbolId, String>,
    /// Actual unboxing getter name of a binary value class.
    pub value_class_getters: rustc_hash::FxHashMap<SymbolId, String>,
    /// Value-class field declarations retained before symbol erasure.
    pub value_class_underlying_types: rustc_hash::FxHashMap<SymbolId, Type>,
    /// Methods whose result was a user value class before erasure.
    pub value_class_results: rustc_hash::FxHashMap<SymbolId, SymbolId>,
    /// Methods with at least one parameter that was a type parameter or an
    /// abstract type member **before** erasure, as a bit per parameter of the
    /// flattened parameter list (bit `i` = parameter `i`; parameters past 32
    /// are not recorded).
    ///
    /// Erasure destroys the only thing that says a subclass method *overrides*
    /// an inherited one rather than overloading it. `def base[T](…, t:
    /// BaseColumnType[U])` erases to `TypedType` in slick's
    /// `MappedColumnTypeFactory` and to `JdbcType` in the `MappedJdbcType`
    /// that implements it, and after erasure those are simply two unrelated
    /// classes. `gen::bridge_overrides` reads this to tell the two apart.
    pub erased_abstract_params: rustc_hash::FxHashMap<SymbolId, u32>,
    /// Value classes compiled from source in this run; see
    /// `erasure::note_source_value_classes`.
    pub source_value_classes: rustc_hash::FxHashSet<SymbolId>,
    /// Classes defined by the units being compiled, as opposed to ones read
    /// from the prelude or the classpath. A library case class keeps its
    /// constructor fields private behind accessors; ours are emitted with the
    /// field public, so the two are read differently.
    pub source_classes: rustc_hash::FxHashSet<SymbolId>,
    /// The receiver temporaries of `c.copy(…)` calls the typer rewrote to
    /// `{ val tmp = c; new CC(…) }` for a case class that is a member of a
    /// class or trait: the backend builds the copy on `tmp.$outer`, the
    /// receiver's own enclosing instance, as nsc's synthetic `copy` does.
    pub copy_receivers: rustc_hash::FxHashSet<SymbolId>,
    /// Scala classes seeded by the shallow classpath reader. Their complete
    /// signature must be adopted before type checking uses their members.
    pub pending_classpath_signatures: rustc_hash::FxHashSet<SymbolId>,
    /// `scala.runtime.LazyRef` & friends, in `prelude_lazyref::CELL_NAMES`
    /// order. The cell classes a method-local `lazy val` is compiled into.
    pub lazy_cells: Vec<SymbolId>,
    /// The synthetic cell `val`s a method-local `lazy val` leaves behind. The
    /// backend gives each one a `new scala/runtime/Lazy…()` instead of the
    /// eager right-hand side.
    pub local_lazy_cells: rustc_hash::FxHashSet<SymbolId>,
    /// Accessor method -> its cell parameter, for the local `lazy val`s
    /// `lazy_local::lazy_locals` rewrote. The backend wraps the accessor's
    /// body in the `initialized` / `synchronized` / `initialize` dance.
    pub local_lazy_accessors: rustc_hash::FxHashMap<SymbolId, SymbolId>,
    /// Methods a hoisted local-`lazy val` accessor `return`s out of. The
    /// `return` moved into the accessor with the initialiser, so the method's
    /// own body no longer shows it, yet the method still has to carry the
    /// `NonLocalReturnControl` handler that catches it.
    pub local_lazy_nlr: rustc_hash::FxHashSet<SymbolId>,
    /// Applications whose named arguments the typer had to move out of the
    /// order they were written in, keyed by `(file index, node id)` of the
    /// `Apply`.
    ///
    /// Entry `i` is the position in the *source* argument list of the argument
    /// now sitting in parameter slot `i`, or `None` for a slot the typer filled
    /// itself (a default, an implicit) and for a by-name parameter, whose
    /// argument must not be evaluated at the call site at all.
    /// [`crate::named_eval_order::restore_named_arg_order`] reads this to put
    /// the evaluation back into source order.
    pub named_arg_order: rustc_hash::FxHashMap<(u32, u32), Vec<Option<usize>>>,
    /// One past the last symbol `install_prelude` built.
    ///
    /// The prelude hand-writes signatures for the part of `scala.*` the typer
    /// reasons about, and those must never be reshaped from a jar. Everything
    /// `scala.*` the prelude does *not* cover (`scala.concurrent.Future`, for
    /// one) arrives from a classfile instead, where a by-name parameter is
    /// indistinguishable from a `Function0`. This is the line between the two.
    pub prelude_end: u32,
    /// Symbols allocated before this index came from the prelude or from the
    /// eager `-cp` classfile scan; from here on they are this run's own
    /// sources (and the pickle members supplied on demand, which carry a
    /// `pickled_origin`). `install_classpath` sets it.
    pub source_start: u32,
    /// Prelude symbols a source definition of the same fully qualified name
    /// has replaced; see [`SymbolTable::shadow_supplied_by_source`].
    ///
    /// The symbols stay in `symbols` — their ids are already written into
    /// prelude signatures — but they are unreachable by name: taken out of
    /// their owner's `members`, replaced in every open scope, and skipped by
    /// `find_class_by_jvm`.
    pub prelude_shadowed: rustc_hash::FxHashSet<SymbolId>,
    /// Index in `scopes` of the scope `install_prelude` puts its names in.
    ///
    /// It is the outermost scope that has anything in it and it never pops,
    /// so it is where the auto-imported `scala._` / `java.lang._` / `Predef._`
    /// members live. A source definition in package `scala` has to join them
    /// there: see [`SymbolTable::enter_in_prelude_scope`].
    pub prelude_scope: usize,
    /// The run's own sources define `scala.Predef`, and
    /// `predef_reimport::reimport_source_predef` has imported it over the
    /// prelude's snapshot. Set once, between the signature pass and the body
    /// pass; false for every ordinary program.
    pub predef_superseded: bool,
    /// User-written `unapplySeq`s whose `Option` payload is *not* a `List`.
    ///
    /// The backend reads a sequence pattern's elements off the payload, and
    /// which code it emits depends on the container: a `List` is walked
    /// head/tail, anything else goes through scalac's
    /// `SeqFactory$UnapplySeqWrapper$` (an `Array` through
    /// `Array$UnapplySeqWrapper$`). Erasure has flattened `Option[Seq[A]]` to
    /// a bare `Option` by the time the backend looks, so the answer is
    /// recorded here while the type arguments are still there. Extractors
    /// absent from the map are walked as `List`, which is what `List`'s own
    /// `unapplySeq` and every built-in factory want.
    pub seq_extractor_payload: rustc_hash::FxHashMap<SymbolId, SeqPayload>,
    /// Name-based extractors (nsc's `isEmpty` / `get` protocol, SLS 8.1.8):
    /// an `unapply` whose result is not an `Option` is matched by calling
    /// `isEmpty` and then `get` on whatever it returns. Recorded by the typer
    /// while the result type still has its arguments; see
    /// [`NameBasedUnapply`].
    pub name_based_unapply: rustc_hash::FxHashMap<SymbolId, NameBasedUnapply>,
    /// Product selectors (`_1` … `_N`) an extractor's `get` value is read
    /// through when a pattern gives it N > 1 sub-patterns and the value is
    /// not a `TupleN` (a `TupleN` keeps the backend's tuple path). Keyed by
    /// `(unapply, N)`: the selectors belong to the type `get` yields, which is
    /// fixed for a given extractor and arity.
    pub unapply_selectors: rustc_hash::FxHashMap<(SymbolId, usize), UnapplySelectors>,
    /// `IterableOps[A, CC, C]`'s `CC` and `C` classes for a library
    /// collection class, read off its pickle (`crate::ops_shape`); `None`
    /// once looked up and not answerable.
    pub ops_shapes: rustc_hash::FxHashMap<SymbolId, Option<OpsShape>>,
    /// `jvm_name` -> class-like symbols carrying it, for `classpath::find_by_jvm`,
    /// which used to scan every symbol on every call. See `JvmIndex`.
    pub(crate) jvm_index: std::cell::RefCell<JvmIndex>,
    /// The last `erasure::erase_symbols` pass changed nothing, and nothing has
    /// changed a symbol's type since. The next pass over the same table would
    /// therefore also change nothing, so it is skipped. Cleared by `alloc` and
    /// by the one place in `erasure` that writes a symbol type outside the
    /// pass itself.
    pub erasure_settled: bool,
    /// How many symbols `uncurry::flatten_method_symbols` has already joined
    /// into a single parameter list. It runs once per compilation unit and
    /// only ever appends, so each pass starts here instead of at 0.
    pub(crate) flattened_upto: usize,
    /// Method-owned primitive variants produced after the source pickle.
    pub method_variants: rustc_hash::FxHashMap<SymbolId, Vec<MethodVariant>>,
    /// Type parameters `display_type` has to name their owner for.
    ///
    /// Two distinct type parameters can share a name, and a diagnostic that
    /// prints only the name then says nothing: cats' `IorT` reported
    /// `type mismatch; found: IorT[F, A, B]  required: IorT[F, A, B]`, where
    /// the two `B`s were `IorT`'s own and one belonging to a method the typer
    /// had eta-expanded. nsc disambiguates the same way (`A(in method make)`).
    ///
    /// Set only around rendering one message, and only for the parameters
    /// whose name really is ambiguous there, so that nothing else in the
    /// compiler sees a different type string.
    pub(crate) qualify_tparams: std::cell::RefCell<Vec<SymbolId>>,
    /// `p.T`: an abstract type member seen through a stable term path.
    ///
    /// `Type::TypeMember` carries no prefix, so `p.T` and `q.T` were the same
    /// type and `q.put(p.get)` type-checked. Rather than give every type a
    /// prefix -- one variant reaching conformance, substitution, as-seen-from,
    /// erasure and the pickle at once -- a path-dependent member is a
    /// *symbol*: a deferred `TypeMember` allocated once per (path,
    /// declaration) pair, carrying the declaration's bounds as seen from the
    /// path and printing as `p.T`. Every walk that already handles an abstract
    /// member handles this one unchanged, and a walk that knows nothing about
    /// paths is no less precise than it was.
    ///
    /// Keyed by the chain of term symbols that spells the path (`a.b.c` is
    /// `[a, b, c]`) together with the declaration being projected.
    pub(crate) path_members: rustc_hash::FxHashMap<(Vec<SymbolId>, SymbolId), SymbolId>,
    /// The prefix a class's `i`th parent was written with, when it names an
    /// inner class of a class (`prefix.rs`): `ThisType(C)` for a bare `In`
    /// inside `C`, the path for `extends o.In`. `parents` itself holds the
    /// bare class, which every reader of it expects.
    pub(crate) parent_prefixes: rustc_hash::FxHashMap<(u32, usize), Type>,
    /// The declaration a path-dependent member stands for. Erasure and the
    /// pickle read this so that they see exactly the type they saw before the
    /// typer could tell two paths apart.
    pub(crate) path_member_decl: rustc_hash::FxHashMap<SymbolId, SymbolId>,
    /// nsc's GADT bounds (`Infer.instantiateTypeVar` / `Context.pushTypeBounds`):
    /// while a `case` is typed, a method type parameter the scrutinee mentions
    /// may carry tighter bounds than it declares, because the pattern's type
    /// says what it is there -- `def ev[T](e: E[T]): T = e match { case I(i)
    /// => i }` with `I extends E[Int]` has `T` at `Int..Int` inside that case.
    /// A stack, innermost last, truncated when the case ends; `is_sub_type`
    /// reads it in both directions. Kept out of the symbol itself so that
    /// erasure and member supply never see a bound that exists for one case.
    pub(crate) gadt_bounds: Vec<(SymbolId, Option<Type>, Option<Type>)>,
    /// The path each path-dependent member was projected out of, for the
    /// dependent-method-type substitution in `subst_dependent_paths`.
    pub(crate) path_member_path: rustc_hash::FxHashMap<SymbolId, Vec<SymbolId>>,
    /// The rewritten copy of an anonymous type-lambda alias, per (alias, the
    /// path member being replaced, its replacement). Substituting the same
    /// pair into the same lambda twice has to give the same symbol, or two
    /// spellings of one type would stop comparing equal -- which is the
    /// property the path members themselves exist to provide.
    pub(crate) path_member_lambdas:
        rustc_hash::FxHashMap<(SymbolId, SymbolId), Vec<(Type, SymbolId)>>,
    /// `P#T` where the prefix `P` is still abstract -- a type parameter or a
    /// deferred type member -- so the projection cannot be reduced yet.
    ///
    /// Represented the same way a path-dependent member is, and for the same
    /// reason (see [`SymbolTable::path_members`]): a deferred `TypeMember`
    /// symbol allocated once per (prefix, declaration) pair rather than a new
    /// `Type` variant. The difference is what settles it. A path member is
    /// settled by the path it was written through and never reduces; an
    /// abstract projection reduces the moment its prefix is *instantiated* --
    /// `TableQuery[E <: AbstractTable[_]] extends Query[E, E#TableElementType,
    /// Seq]` at `E := Accounts` is `Query[Accounts, (String, Int), Seq]` --
    /// which is what [`SymbolTable::subst_projections`] does.
    ///
    /// Keyed by the prefix's symbol and the declaration being projected.
    pub(crate) abs_projections: rustc_hash::FxHashMap<(SymbolId, SymbolId), SymbolId>,
    /// `(prefix symbol, declaration)` for each symbol in `abs_projections`.
    pub(crate) abs_projection_of: rustc_hash::FxHashMap<SymbolId, (SymbolId, SymbolId)>,
    /// How many times a symbol has been handed out for mutation
    /// ([`SymbolTable::get_mut`] and the few places that index `symbols`
    /// directly). Read-only caches over the symbol graph key themselves on it,
    /// so any change at all -- whether or not it could have mattered -- throws
    /// them away rather than risking a stale answer. See [`LinCache`].
    pub(crate) mutation_gen: std::cell::Cell<u64>,
    /// [`crate::lin::linearize`]'s answers for the current `mutation_gen`.
    pub(crate) lin_cache: std::cell::RefCell<LinCache>,
    /// [`SymbolTable::base_type_args`]'s answers for the current
    /// `mutation_gen`.
    pub(crate) bta_cache: std::cell::RefCell<BtaCache>,
    /// The answer for a receiver that names no class, handed out by reference
    /// so the hot path never allocates for it.
    pub(crate) empty_base_type_args: std::rc::Rc<BaseTypeArgs>,
}

/// Memo for [`SymbolTable::base_type_args`], with the same validity rule as
/// [`LinCache`]: an entry lives until any symbol is handed out for mutation.
///
/// The walk is asked for the same applied class over and over -- 44 million
/// calls in a gitbucket build, and after the linearization cache the largest
/// remaining entry in that profile. The key is the pair the walk is a function
/// of: the class and the arguments it is applied at. Buckets are per class and
/// scanned, because a class is asked about at a handful of instantiations and a
/// `Vec<Type>` cannot be hashed (`Type` holds an `f64`).
#[derive(Default)]
pub(crate) struct BtaCache {
    /// The generation `map` was filled at.
    gen: u64,
    map: rustc_hash::FxHashMap<u32, Vec<BtaEntry>>,
    /// Total entries, so the cache can be dropped rather than grow without
    /// bound over a long run.
    len: usize,
}

/// One answered instantiation: the arguments the class was applied at, and the
/// base-class map that came out.
type BtaEntry = (Vec<Type>, std::rc::Rc<BaseTypeArgs>);

/// Past this many instantiations of one class the bucket stops growing: the
/// scan is linear, and a class asked about at hundreds of instantiations is
/// one the cache cannot help anyway.
const BTA_BUCKET_MAX: usize = 16;
/// Past this many entries in total the cache is emptied.
const BTA_CACHE_MAX: usize = 100_000;

/// Memo for [`crate::lin::linearize`], shared by every call made while the
/// symbol graph stands still.
///
/// `linearize` is a pure function of the `extends` graph and is called about
/// 45 million times in a gitbucket build (812 million `Lin::lin` node visits,
/// 14 s of CPU: the single largest entry in the profile), almost always on a
/// class whose ancestry has not changed since the last time it was asked.
/// `Lin`'s own memo only ever lived for one call, so a diamond was re-expanded
/// once per caller.
///
/// Correctness rests on one rule: an entry is only valid while
/// [`SymbolTable::mutation_gen`] is the one it was filled at. A namer or a
/// pickle completion that rewrites a parent list bumps that counter (through
/// `get_mut`), and the whole cache is dropped. Entries a truncated walk
/// produced are never published, exactly as `Lin`'s own memo never keeps them.
#[derive(Clone, Debug, Default)]
pub(crate) struct LinCache {
    /// The generation `map` was filled at.
    gen: u64,
    map: rustc_hash::FxHashMap<u32, std::rc::Rc<Vec<SymbolId>>>,
}

/// Reverse index from `jvm_name` to the class-like symbols that have it.
///
/// Built lazily: `symbols` only ever grows, so a call indexes whatever was
/// appended since the last one and stops. `SymKind` is never reassigned after
/// `alloc`, so "class-like" is decided once here; `jvm_name` *is* reassigned
/// (`apply_java_class_meta` renames a stub once the class file is read), which
/// is why `SymbolTable::set_jvm_name` is the only supported way to write it and
/// why lookups re-check the name they find.
#[derive(Clone, Debug, Default)]
pub(crate) struct JvmIndex {
    /// How many entries of `symbols` have been folded into `map`.
    upto: usize,
    map: HashMap<String, Vec<SymbolId>>,
}

/// How a pattern reads a name-based extractor's result.
#[derive(Clone, Debug)]
pub struct NameBasedUnapply {
    /// The nullary `isEmpty` member of the result type.
    pub is_empty: SymbolId,
    /// The nullary `get` member of the result type.
    pub get: SymbolId,
    /// A synthetic local that holds the result while `isEmpty` and `get` are
    /// called on it (the backend gives it a slot per use).
    pub result_tmp: SymbolId,
    /// The result type's class, which the calls are made on.
    pub result_class: SymbolId,
}

/// The classes a collection's `IterableOps[A, CC, C]` binds `CC` and `C` to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpsShape {
    pub cc: SymbolId,
    pub c: SymbolId,
}

/// The product selectors a multi-pattern extractor reads its `get` value
/// through; see [`SymbolTable::unapply_selectors`].
#[derive(Clone, Debug)]
pub struct UnapplySelectors {
    /// `_1` … `_N`, in order.
    pub selectors: Vec<SymbolId>,
    /// A synthetic local holding the `get` value while they are read.
    pub tmp: SymbolId,
    /// The class of the `get` value, which the selectors are read on.
    pub class: SymbolId,
}

/// The container a `unapplySeq` hands back inside its `Option`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SeqPayload {
    /// `Option[Seq[A]]`, `Option[IndexedSeq[A]]`, `Option[Vector[A]]`, …
    Seq,
    /// `Option[Array[A]]`.
    Array,
}

impl SymbolTable {
    pub fn new() -> Self {
        let mut st = SymbolTable {
            parent_outer_modules: HashMap::default(),
            binary_alias_prefixes: HashMap::default(),
            binary_read: rustc_hash::FxHashSet::default(),
            sam_known_overrides: rustc_hash::FxHashMap::default(),
            mutation_gen: std::cell::Cell::new(0),
            lin_cache: std::cell::RefCell::new(LinCache::default()),
            bta_cache: std::cell::RefCell::new(BtaCache::default()),
            empty_base_type_args: std::rc::Rc::new(BaseTypeArgs::default()),
            symbols: vec![Symbol {
                id: SymbolId(0),
                name: "<none>".into(),
                owner: SymbolId(0),
                kind: SymKind::NoSymbol,
                flags: Flags::EMPTY,
                ty: Type::NoType,
                members: vec![],
                jvm_name: String::new(),
                binary_outer_desc: None,
                intrinsic: Intrinsic::None,
                params: vec![],
                paramss: vec![],
                pickle_clauses: vec![],
                parameterless_method: None,
                ctor_fields: vec![],
                parents: vec![],
                default_rhs: None,
                tparams: vec![],
                children: vec![],
                self_type: None,
                self_alias: None,
                private_within: None,
                access_widened: false,
                low_priority: false,
                java_object_field: false,
                annotations: vec![],
                bound_lo: None,
                bound_hi: None,
                is_type_alias: false,
                is_pattern_skolem: false,
                captures: vec![],
                macro_impl: None,
                declaring_class: String::new(),
                declaring_is_interface: false,
                pickled_origin: String::new(),
                pickled_owner_bases: Vec::new(),
                abstract_override: false,
                super_accessor: false,
                deferred_val: false,
                deferred_method: false,
                via_accessor: false,
                specialized: None,
                unspecialized: false,
                local_scope: None,
            }],
            scopes: vec![Scope::default()],
            root: SymbolId(0),
            scala_pkg: SymbolId(0),
            predef: SymbolId(0),
            singleton_sym: SymbolId::NONE,
            any_sym: SymbolId(0),
            anyref_sym: SymbolId(0),
            anyval_sym: SymbolId(0),
            int_sym: SymbolId(0),
            byte_sym: SymbolId(0),
            short_sym: SymbolId(0),
            char_sym: SymbolId(0),
            long_sym: SymbolId(0),
            float_sym: SymbolId(0),
            double_sym: SymbolId(0),
            boolean_sym: SymbolId(0),
            unit_sym: SymbolId(0),
            string_sym: SymbolId(0),
            array_sym: SymbolId(0),
            option_sym: SymbolId(0),
            some_sym: SymbolId(0),
            none_sym: SymbolId(0),
            list_sym: SymbolId(0),
            nil_sym: SymbolId(0),
            cons_sym: SymbolId(0),
            object_sym: SymbolId(0),
            owner: SymbolId(0),
            this_class: SymbolId(0),
            super_accessor_targets: rustc_hash::FxHashMap::default(),
            inherited_method_implementations: rustc_hash::FxHashSet::default(),
            method_override_families: rustc_hash::FxHashSet::default(),
            method_overload_pairs: rustc_hash::FxHashSet::default(),
            constructor_parameter_names: rustc_hash::FxHashMap::default(),
            value_class_getters: rustc_hash::FxHashMap::default(),
            value_class_underlying_types: rustc_hash::FxHashMap::default(),
            value_class_results: rustc_hash::FxHashMap::default(),
            value_class_terms: rustc_hash::FxHashMap::default(),
            erased_abstract_params: rustc_hash::FxHashMap::default(),
            source_value_classes: rustc_hash::FxHashSet::default(),
            source_classes: rustc_hash::FxHashSet::default(),
            copy_receivers: rustc_hash::FxHashSet::default(),
            pending_classpath_signatures: rustc_hash::FxHashSet::default(),
            lazy_cells: Vec::new(),
            local_lazy_cells: rustc_hash::FxHashSet::default(),
            local_lazy_accessors: rustc_hash::FxHashMap::default(),
            local_lazy_nlr: rustc_hash::FxHashSet::default(),
            named_arg_order: rustc_hash::FxHashMap::default(),
            prelude_end: 0,
            source_start: 0,
            prelude_shadowed: rustc_hash::FxHashSet::default(),
            prelude_scope: 0,
            predef_superseded: false,
            seq_extractor_payload: rustc_hash::FxHashMap::default(),
            name_based_unapply: rustc_hash::FxHashMap::default(),
            unapply_selectors: rustc_hash::FxHashMap::default(),
            ops_shapes: rustc_hash::FxHashMap::default(),
            jvm_index: std::cell::RefCell::new(JvmIndex::default()),
            erasure_settled: false,
            flattened_upto: 0,
            method_variants: rustc_hash::FxHashMap::default(),
            qualify_tparams: std::cell::RefCell::new(Vec::new()),
            path_members: rustc_hash::FxHashMap::default(),
            parent_prefixes: rustc_hash::FxHashMap::default(),
            path_member_decl: rustc_hash::FxHashMap::default(),
            gadt_bounds: Vec::new(),
            path_member_path: rustc_hash::FxHashMap::default(),
            path_member_lambdas: rustc_hash::FxHashMap::default(),
            abs_projections: rustc_hash::FxHashMap::default(),
            abs_projection_of: rustc_hash::FxHashMap::default(),
        };
        st.root = st.alloc(
            "<_root_>",
            SymbolId(0),
            SymKind::Package,
            Flags::PACKAGE,
            "scala/runtime",
        );
        st.owner = st.root;
        st
    }

    pub fn alloc(
        &mut self,
        name: impl Into<String>,
        owner: SymbolId,
        kind: SymKind,
        flags: Flags,
        jvm_name: impl Into<String>,
    ) -> SymbolId {
        let id = SymbolId(self.symbols.len() as u32);
        // A symbol that appears after an erasure pass has an un-erased type,
        // so the next pass has work to do again.
        self.erasure_settled = false;
        self.symbols.push(Symbol {
            id,
            name: name.into(),
            owner,
            kind,
            flags,
            ty: Type::NoType,
            members: vec![],
            jvm_name: jvm_name.into(),
            binary_outer_desc: None,
            intrinsic: Intrinsic::None,
            params: vec![],
            paramss: vec![],
            pickle_clauses: vec![],
            parameterless_method: None,
            ctor_fields: vec![],
            parents: vec![],
            default_rhs: None,
            tparams: vec![],
            children: vec![],
            self_type: None,
            self_alias: None,
            private_within: None,
            access_widened: false,
            low_priority: false,
            java_object_field: false,
            annotations: vec![],
            bound_lo: None,
            bound_hi: None,
            is_type_alias: false,
            is_pattern_skolem: false,
            captures: vec![],
            macro_impl: None,
            declaring_class: String::new(),
            declaring_is_interface: false,
            pickled_origin: String::new(),
            pickled_owner_bases: Vec::new(),
            abstract_override: false,
            super_accessor: false,
            deferred_val: false,
            deferred_method: false,
            via_accessor: false,
            specialized: None,
            unspecialized: false,
            local_scope: None,
        });
        if !owner.is_none() && owner.0 as usize <= self.symbols.len() {
            if let Some(ow) = self.symbols.get_mut(owner.0 as usize) {
                ow.members.push(id);
            }
        }
        id
    }

    pub fn get(&self, id: SymbolId) -> &Symbol {
        &self.symbols[id.0 as usize]
    }

    pub fn get_mut(&mut self, id: SymbolId) -> &mut Symbol {
        self.note_mutation();
        &mut self.symbols[id.0 as usize]
    }

    /// Say that a symbol is about to change, so every read-only cache over the
    /// symbol graph stops trusting what it holds. `get_mut` calls this; the
    /// handful of places that reach into `symbols` directly call it too.
    #[inline]
    pub(crate) fn note_mutation(&self) {
        self.mutation_gen
            .set(self.mutation_gen.get().wrapping_add(1));
    }

    /// `cls`'s linearization from [`LinCache`], if it was computed since the
    /// last change to any symbol.
    #[inline]
    pub(crate) fn cached_linearization(&self, cls: SymbolId) -> Option<std::rc::Rc<Vec<SymbolId>>> {
        let gen = self.mutation_gen.get();
        let cache = self.lin_cache.borrow();
        if cache.gen != gen {
            return None;
        }
        cache.map.get(&cls.0).cloned()
    }

    /// Publish the path-independent results of one linearization walk.
    pub(crate) fn publish_linearizations(
        &self,
        entries: rustc_hash::FxHashMap<u32, Vec<SymbolId>>,
    ) {
        if entries.is_empty() {
            return;
        }
        let gen = self.mutation_gen.get();
        let mut cache = self.lin_cache.borrow_mut();
        if cache.gen != gen {
            cache.gen = gen;
            cache.map.clear();
        }
        for (k, v) in entries {
            cache.map.entry(k).or_insert_with(|| std::rc::Rc::new(v));
        }
    }

    /// [`Self::base_type_args`]'s answer for this exact pair, if it was
    /// computed since the last change to any symbol.
    fn cached_base_type_args(
        &self,
        sym: SymbolId,
        args: &[Type],
    ) -> Option<std::rc::Rc<BaseTypeArgs>> {
        let gen = self.mutation_gen.get();
        let cache = self.bta_cache.borrow();
        if cache.gen != gen {
            return None;
        }
        cache
            .map
            .get(&sym.0)?
            .iter()
            .find(|(a, _)| a.as_slice() == args)
            .map(|(_, v)| v.clone())
    }

    fn cache_base_type_args(&self, sym: SymbolId, args: &[Type], out: &std::rc::Rc<BaseTypeArgs>) {
        let gen = self.mutation_gen.get();
        let mut cache = self.bta_cache.borrow_mut();
        if cache.gen != gen {
            cache.gen = gen;
            cache.map.clear();
            cache.len = 0;
        }
        if cache.len >= BTA_CACHE_MAX {
            cache.map.clear();
            cache.len = 0;
        }
        let bucket = cache.map.entry(sym.0).or_default();
        if bucket.len() >= BTA_BUCKET_MAX || bucket.iter().any(|(a, _)| a.as_slice() == args) {
            return;
        }
        bucket.push((args.to_vec(), out.clone()));
        cache.len += 1;
    }

    /// Copy whatever `@specialized` / `@unspecialized` a definition's
    /// modifiers carry onto its symbol. See [`Symbol::specialized`].
    ///
    /// The parser has already normalised import renames and has already
    /// dropped both annotations under `-no-specialization`, so an annotation
    /// reaching here is one nsc would act on.
    pub fn record_specialization(&mut self, id: SymbolId, annots: &[scala_rs_parser::Tree]) {
        for a in annots {
            if let Some(types) = scala_rs_parser::specialized_types(a) {
                self.get_mut(id).specialized = Some(types);
            } else if scala_rs_parser::is_unspecialized(a) {
                self.get_mut(id).unspecialized = true;
            }
        }
    }

    /// The method-owned selections that the first post-pickle specializer can
    /// actually emit.  Pickling runs before `specialize_method_defs`, so the
    /// writer cannot consult `method_variants`; it must use the same stable
    /// eligibility boundary here instead.  Returning `None` suppresses both
    /// the annotation and nsc's SPECIALIZED bit, leaving a generic fallback
    /// declaration with no promise of a missing JVM entry.
    pub fn method_specialization_for_pickle(
        &self,
        type_param: SymbolId,
    ) -> Option<scala_rs_parser::SpecializedTypes> {
        let selected = self.get(type_param).specialized?;
        let method_id = self.get(type_param).owner;
        let method = self.get(method_id);
        if method.kind != SymKind::Method
            || method.unspecialized
            || method.flags.contains(Flags::ABSTRACT)
            || method.flags.contains(Flags::NATIVE)
            || method.tparams.len() != 1
        {
            return None;
        }
        let owner = self.get(method.owner);
        let eligible_owner = matches!(owner.kind, SymKind::ModuleClass)
            || owner.flags.contains(Flags::FINAL)
            || method.flags.contains(Flags::PRIVATE)
            || method.flags.contains(Flags::FINAL);
        if !eligible_owner {
            return None;
        }
        let lower = self
            .get(type_param)
            .bound_lo
            .clone()
            .unwrap_or(Type::Nothing);
        let upper = self.get(type_param).bound_hi.clone().unwrap_or(Type::Any);
        let supported: Vec<_> = selected
            .iter()
            .filter(|ty| matches!(ty, SpecializedType::Int | SpecializedType::Long))
            .filter(|ty| {
                let primitive = match ty {
                    SpecializedType::Int => Type::Int,
                    SpecializedType::Long => Type::Long,
                    _ => unreachable!("the method slice filters Int and Long first"),
                };
                self.is_sub_type(&lower, &primitive) && self.is_sub_type(&primitive, &upper)
            })
            .collect();
        (!supported.is_empty()).then(|| SpecializedTypes::of(&supported))
    }

    pub fn enter_in_current(&mut self, name: &str, id: SymbolId) {
        self.scopes.last_mut().unwrap().enter(name, id);
    }

    /// [`Self::enter_in_current`] at an SLS 2 precedence other than
    /// "definition": what an import brings in does not hide a definition, and
    /// a wildcard import does not hide an explicit one.
    pub fn enter_in_current_ranked(&mut self, name: &str, id: SymbolId, rank: BindRank) {
        self.scopes.last_mut().unwrap().enter_ranked(name, id, rank);
    }

    /// [`Self::enter_in_current_ranked`] for what an `import` clause brings
    /// in, tagged with which clause it was. See [`Binding::origin`].
    pub fn enter_import_in_current(
        &mut self,
        name: &str,
        id: SymbolId,
        rank: BindRank,
        origin: u64,
    ) {
        self.scopes
            .last_mut()
            .unwrap()
            .enter_binding(name, id, rank, origin);
    }

    /// The precedence a reference here would bind `name` at: the best rank in
    /// the innermost scope that binds it at all, or `None` when nothing does.
    pub fn bind_rank(&self, name: &str) -> Option<BindRank> {
        for sc in self.scopes.iter().rev() {
            if let Some(r) = sc.lookup_ranked(name).iter().map(|b| b.rank).min() {
                return Some(r);
            }
        }
        None
    }

    /// Whether a *term* reference to `name` here is SLS 2's ambiguous
    /// reference: the innermost scope that binds it in the term namespace
    /// binds it, at one precedence, through two different `import` clauses.
    ///
    /// Only bindings that are actually *visible* here count, which is nsc's
    /// `qualifies` filter in `Context.lookupSymbol`. `pos/t2133` writes
    /// `import bip._; import bar._` where `bar.fn` is `private[this]`: there
    /// is one candidate, not two, and scalac compiles it.
    pub fn ambiguous_term_import(&self, name: &str) -> bool {
        for sc in self.scopes.iter().rev() {
            let slot = sc.lookup_ranked(name);
            if !slot.iter().any(|b| self.is_term_namespace(b.sym)) {
                continue;
            }
            return two_imports_tie(slot, |s| self.is_term_namespace(s) && self.visible_here(s));
        }
        false
    }

    /// SLS 2's *other* ambiguous reference: a definition and an `import`
    /// clause at a **deeper nesting level** than it.
    ///
    /// Precedence alone would let the definition win (level 1 beats 2 and 3),
    /// but nsc only consults an import that is *inside* the scope the
    /// definition was found in -- `Contexts.lookupSymbol` walks the import
    /// list while `imp1.depth > symbolDepth` -- and when such an import also
    /// offers the name, `defSym` and `impSym` together are
    /// `ambiguousDefnAndImport` rather than a choice. Returns the definition
    /// and the [`Binding::origin`] of the import clause, for the message.
    ///
    /// `SymbolTable::scopes` carries the nesting level as its own index, so
    /// "deeper" is a comparison of two scope indices: the innermost scope
    /// binding `name` at [`BindRank::Definition`] against the innermost one
    /// binding it through a written `import`. Equal levels are *not*
    /// ambiguous -- `class C { import p._; def X = 1; def f = X }` is one
    /// scope in nsc as well as here, and scalac compiles it.
    ///
    /// Three things keep this from over-reaching, and each was verified
    /// against scalac 2.13.16 rather than reasoned about:
    ///
    /// * only *visible* bindings count, nsc's `qualifies` filter -- the same
    ///   rule [`Self::ambiguous_term_import`] needs for `pos/t2133`;
    /// * only a binding a written clause made (`origin != 0`) is an import.
    ///   The implicit `scala._` / `java.lang._` every source carries, and
    ///   every name this compiler enters into whatever scope happens to be
    ///   current while completing something, are not clauses the program
    ///   wrote, and nsc's root imports sit at depth 0 where nothing can be
    ///   deeper than them;
    /// * [`BindRank::PackageElsewhere`] is nsc's level 4
    ///   (`isPackageOwnedInDifferentUnit`), the documented exception: there
    ///   the import simply wins. It is not `Definition`, so it is never the
    ///   left-hand side of this comparison.
    ///
    /// And the definition must come from a scope the *program* opened. The
    /// prelude's scopes, up to and including [`Self::prelude_scope`], are
    /// this compiler's model of `java.lang._` / `scala._` / `Predef._` being
    /// open around every unit -- root imports, which nsc keeps at depth 0
    /// where nothing can be deeper than them and which are never `defSym`.
    /// Counting them made `import scala.util.Try` inside a method ambiguous
    /// against "package scala" (three `tests/conform` fixtures), which is a
    /// program scalac compiles.
    pub fn ambiguous_defn_and_deeper_import(&self, name: &str) -> Option<(SymbolId, u64)> {
        let mut defn: Option<(usize, SymbolId)> = None;
        let mut imported: Option<(usize, u64)> = None;
        for (depth, sc) in self.scopes.iter().enumerate().skip(self.prelude_scope + 1) {
            for b in sc.lookup_ranked(name) {
                if !self.is_term_namespace(b.sym) || !self.visible_here(b.sym) {
                    continue;
                }
                match b.rank {
                    BindRank::Definition => defn = Some((depth, b.sym)),
                    BindRank::Explicit | BindRank::Wildcard if b.origin != 0 => {
                        imported = Some((depth, b.origin))
                    }
                    _ => {}
                }
            }
        }
        let (def_depth, def_sym) = defn?;
        let (imp_depth, origin) = imported?;
        (imp_depth > def_depth).then_some((def_sym, origin))
    }

    /// Whether a `private` member is reachable from the class being typed.
    /// SLS 5.2: only from inside its own owner (or something nested in it).
    fn visible_here(&self, s: SymbolId) -> bool {
        if !self.private_to_owner(s) {
            return true;
        }
        let owner = self.get(s).owner;
        let mut cur = self.this_class;
        while !cur.is_none() {
            if cur == owner {
                return true;
            }
            let up = self.get(cur).owner;
            if up == cur {
                break;
            }
            cur = up;
        }
        false
    }

    /// Auto-import a source definition that belongs in package `scala`.
    ///
    /// nsc opens `java.lang._`, `scala._` and `Predef._` around every
    /// compilation unit. The prelude models the `scala._` half by copying the
    /// package's members into its own scope once, at install time — a
    /// snapshot, which a source `Tuple9.scala` compiled in the same run
    /// arrives too late for. In `--no-scala-library` mode the prelude does
    /// not build `Tuple3` and up at all (there is no such class in the
    /// private runtime), so `Ordering.scala`, three packages away, could not
    /// see the source `Tuple9` under any spelling: `class_sym_of` answered
    /// `None` for `(T1, …, T9)` and every `x._1` on it was "not a member".
    pub fn enter_in_prelude_scope(&mut self, name: &str, id: SymbolId) {
        let i = self.prelude_scope;
        if let Some(sc) = self.scopes.get_mut(i) {
            sc.enter(name, id);
        }
    }

    /// A source definition of a name the prelude also supplies **replaces**
    /// the prelude's symbol.
    ///
    /// The typer knows the standard library as the hand-written
    /// `prelude*.rs` signature tables, so compiling `src/library` itself asks
    /// it to typecheck source definitions of the very names it already
    /// believes it knows. Without this, `trait IterableOnce` in
    /// `package scala.collection` merely joins the prelude's `IterableOnce`
    /// in the package's member list, the prelude's symbol is the one every
    /// lookup returns (it was allocated first), and the file that *defines*
    /// `iterator` reports `value iterator is not a member of IterableOnce[A]`.
    ///
    /// "Replaces" means unreachable by name: out of the owner's `members`
    /// (so `lookup_member` and the per-package scope built from it stop
    /// seeing it), out of every open scope — the prelude enters names like
    /// `IterableOnce` and `<:<` into the base scope by hand, and that base
    /// scope stays open for the whole run, so an entry there would outlive
    /// any shadowing — and skipped by `find_class_by_jvm`. The symbol itself
    /// stays: its id is written into prelude signatures that are still in
    /// use, and rewriting those is not something a namer can do.
    ///
    /// Prelude declarations and eagerly loaded classpath declarations are
    /// replaced. A source recompile supersedes its previous classfiles; two
    /// source declarations remain distinct and can still be diagnosed.
    pub fn shadow_supplied_by_source(&mut self, id: SymbolId) {
        if id.is_none() || id.0 < self.prelude_end {
            return;
        }
        let owner = self.get(id).owner;
        if owner.is_none() {
            return;
        }
        let name = self.get(id).name.clone();
        let kind = self.get(id).kind;
        let prelude_end = self.prelude_end;
        let victims: Vec<SymbolId> = self
            .get(owner)
            .members
            .iter()
            .copied()
            .filter(|&m| {
                m != id
                    && (m.0 < prelude_end
                        || self.pending_classpath_signatures.contains(&m)
                        || (self.get(m).kind == SymKind::Module
                            && self
                                .pending_classpath_signatures
                                .contains(&self.module_class_of(m))))
                    && self.get(m).name == name
                    && shadowable_kind(kind, self.get(m).kind)
            })
            .collect();
        if victims.is_empty() {
            return;
        }
        self.get_mut(owner).members.retain(|m| !victims.contains(m));
        for sc in self.scopes.iter_mut() {
            sc.replace(&name, &victims, id);
        }
        self.prelude_shadowed.extend(victims);
    }

    /// Fold a package object's members into its package, the way nsc's
    /// `openPackageModule` does: a name the package object declares
    /// **unlinks** whatever the package already had under it, in that name's
    /// own namespace, and only then are the package object's members entered.
    ///
    /// A package object's members *are* the package's members (SLS 9.3), so
    /// two entries under one name are not an overload set — they are one name
    /// supplied twice by routes that stand in no `extends` relation, which no
    /// ordering rule can reduce. Compiling `src/library`, `Nil` offered the
    /// prelude's `Module` owned by package `scala` beside the `Term`
    /// `scala/package.scala` declares as `val Nil =
    /// scala.collection.immutable.Nil`, and every use printed
    /// `<overload Nil$ | Nil$>` — the same type printed twice, which is what a
    /// duplicate *supply* looks like rather than an overload.
    ///
    /// nsc resolves it in the package object's favour, with a comment saying
    /// it is provisional ("for now the symbol in the package module takes
    /// precedence"), and real scalac 2.13.16 is measurably that way round:
    /// `package p { object Impl }` beside `package object p { val Impl: Int =
    /// 42 }` compiles and prints `42`, and writing `val Impl = p.Impl`
    /// instead is "recursive value Impl needs type" — the package object's
    /// entry is the only `p.Impl` there is. Both are
    /// `tests/fixtures/pkgobjdup_*.scala`.
    ///
    /// **Two passes, not one.** nsc unlinks over the package's existing decls
    /// before entering anything, and the order is load-bearing: done member by
    /// member, `package object math`'s own `def abs(x: Int)` and `def abs(x:
    /// Double)` unlink *each other* as the second is folded, and the library
    /// went from 836 errors to 924 (twenty of them `found: Double required:
    /// Int`). `already_folded` is what keeps the pending re-fold — which
    /// re-runs this with the package object's members already in the package —
    /// from doing the same thing.
    pub fn fold_package_object_members(&mut self, pkg: SymbolId, mems: &[SymbolId]) {
        if pkg.is_none() || self.get(pkg).kind != SymKind::Package {
            return;
        }
        let already_folded: rustc_hash::FxHashSet<SymbolId> = mems.iter().copied().collect();
        // Scala's two namespaces are separate, so `val List` displaces a
        // module and `type List[+A] = …` displaces a class, and each leaves
        // the other alone. `scala/package.scala` writes both lines, and both
        // halves are needed: with only the term half, `var res: List[Any] =
        // Nil` read `List` as the prelude's class and `Nil` as the package
        // object's `val` -- whose `case object Nil extends List[Nothing]`
        // names the *source* `List` -- and the two `List`s do not conform
        // (measured: four such mismatches, and eleven errors in all).
        let mut terms: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
        let mut types: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
        for &m in mems {
            let s = self.get(m);
            if is_term_kind(s.kind) {
                terms.insert(s.name.clone());
            } else if is_type_kind(s.kind) {
                types.insert(s.name.clone());
            }
        }
        let victims: Vec<SymbolId> = self
            .get(pkg)
            .members
            .iter()
            .copied()
            .filter(|&m| {
                if already_folded.contains(&m) {
                    return false;
                }
                let s = self.get(m);
                (is_term_kind(s.kind) && terms.contains(&s.name))
                    || (is_type_kind(s.kind) && types.contains(&s.name))
            })
            .collect();
        if !victims.is_empty() {
            self.get_mut(pkg).members.retain(|m| !victims.contains(m));
            let prelude_end = self.prelude_end;
            for v in victims {
                let name = self.get(v).name.clone();
                let winner = mems
                    .iter()
                    .copied()
                    .find(|&m| self.get(m).name == name)
                    .unwrap_or(v);
                for sc in self.scopes.iter_mut() {
                    sc.replace(&name, &[v], winner);
                }
                // Only a *prelude* victim stops being the class of its binary
                // name. A displaced source or classfile symbol is still a real
                // class `find_class_by_jvm` has to answer with: it has lost a
                // *name*, not its identity, and `p/Impl$.class` is still
                // emitted and still loaded.
                if v.0 < prelude_end {
                    self.prelude_shadowed.insert(v);
                }
            }
        }
        for &mem in mems {
            if !self.get(pkg).members.contains(&mem) {
                self.get_mut(pkg).members.push(mem);
            }
        }
    }

    /// Record `import owner._` in the innermost scope.
    pub fn enter_wildcard_in_current(&mut self, owner: SymbolId, hidden: &[String]) {
        self.scopes
            .last_mut()
            .unwrap()
            .enter_wildcard(owner, hidden);
    }

    /// Owners a wildcard import offers `name`, innermost scope first.
    pub fn wildcard_owners_for(&self, name: &str) -> Vec<SymbolId> {
        let mut out = Vec::new();
        for sc in self.scopes.iter().rev() {
            for w in sc.wildcards() {
                if w.offers(name) && !out.contains(&w.owner) {
                    out.push(w.owner);
                }
            }
        }
        out
    }

    pub fn push_scope(&mut self) {
        self.scopes.push(Scope::default());
    }

    pub fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    pub fn lookup(&self, name: &str) -> Vec<SymbolId> {
        for sc in self.scopes.iter().rev() {
            let found = sc.lookup(name);
            if !found.is_empty() {
                return found;
            }
        }
        Vec::new()
    }

    /// Whether `id` is a type the `scala._` / `java.lang._` wildcard imports
    /// every source carries would supply -- the owner test, not identity
    /// against `int_sym` and friends, because scala/scala's own `src/library`
    /// declares `scala.Int`, `scala.Any` and the rest as ordinary source
    /// classes and a reference to *those* must still resolve to the builtin
    /// `Type`. Used by `Checker::builtin_type_shadowed`, which is the only
    /// caller and only ever asks about the fixed set of builtin simple names.
    pub fn is_prelude_scope_type(&self, id: SymbolId) -> bool {
        let owner = self.get(id).owner;
        if owner.is_none() {
            return false;
        }
        owner == self.scala_pkg || self.get(owner).jvm_name == "java/lang"
    }

    /// Look a name up in the *type* namespace. Scala keeps terms and types in
    /// separate namespaces, so `val F = asyncF` inside a class parameterized by
    /// `F[_]` must not hide the type parameter from `val u: F[Unit]`. A scope
    /// that binds the name only as a term is therefore skipped, and the search
    /// continues outward.
    ///
    /// A module is only a *fallback*: nsc names the module class of `object X`
    /// `X$`, so an inherited `object JdbcType` never shadows the top-level
    /// `trait JdbcType[T]` in type position. `X` alone still resolves to the
    /// module when nothing in the type namespace carries that name.
    pub fn lookup_type(&self, name: &str) -> Vec<SymbolId> {
        let mut module_fallback: Vec<SymbolId> = Vec::new();
        for sc in self.scopes.iter().rev() {
            let slot = sc.lookup_ranked(name);
            if slot.is_empty() {
                continue;
            }
            // A module can share this scope with the real type-namespace
            // symbol (an object and, elsewhere, a `type` alias of the same
            // name both named `NonEmptyLazyList` -- cats' `Newtype`
            // encoding). The module is a fallback only, so it must not
            // ride along in the answer: the caller picks the *first*
            // match of a kind it accepts, and an unfiltered vector let the
            // module win over the alias by accident of insertion order.
            let types = best_ranked(slot, |s| self.is_type_namespace(s));
            if !types.is_empty() {
                return types;
            }
            if module_fallback.is_empty() {
                // The whole slot at the precedence the *module* binds at:
                // `lookup_type` has always handed its callers the slot rather
                // than the modules alone here.
                if let Some(best) = slot
                    .iter()
                    .filter(|b| self.is_module_like(b.sym))
                    .map(|b| b.rank)
                    .min()
                {
                    module_fallback = slot
                        .iter()
                        .filter(|b| b.rank == best)
                        .map(|b| b.sym)
                        .collect();
                }
            }
        }
        module_fallback
    }

    /// [`Self::bind_rank`] restricted to the type namespace: the precedence
    /// the innermost scope that binds `name` as a *type* binds it at. Pairs
    /// with [`Self::has_real_type_entry`], which answers the same question
    /// without the rank.
    pub fn type_bind_rank(&self, name: &str) -> Option<BindRank> {
        for sc in self.scopes.iter().rev() {
            let r = sc
                .lookup_ranked(name)
                .iter()
                .filter(|b| self.is_type_namespace(b.sym))
                .map(|b| b.rank)
                .min();
            if r.is_some() {
                return r;
            }
        }
        None
    }

    /// Whether `name` already answers to a genuine type-namespace symbol --
    /// not merely the module `lookup_type` offers as a fallback when nothing
    /// else does. Callers deciding whether it is still worth *looking for* a
    /// type-namespace answer (a source package member, a pickled alias) must
    /// not stop just because `lookup_type` is non-empty: that can be the
    /// fallback alone.
    pub fn has_real_type_entry(&self, name: &str) -> bool {
        for sc in self.scopes.iter().rev() {
            let slot = sc.lookup_ranked(name);
            if slot.is_empty() {
                continue;
            }
            if slot.iter().any(|b| self.is_type_namespace(b.sym)) {
                return true;
            }
        }
        false
    }

    /// A prelude type is a default import, not a completed user wildcard.
    /// Explicit source imports carry an origin and must retain their rank.
    pub(crate) fn has_only_default_type_binding(&self, name: &str) -> bool {
        for scope in self.scopes.iter().rev() {
            let bindings: Vec<_> = scope
                .lookup_ranked(name)
                .iter()
                .filter(|b| self.is_type_namespace(b.sym))
                .collect();
            if !bindings.is_empty() {
                return bindings.iter().all(|b| {
                    b.origin == 0 && b.sym.0 < self.prelude_end && self.is_prelude_scope_type(b.sym)
                });
            }
        }
        false
    }

    /// Look a name up in the *term* namespace, the mirror of `lookup_type`.
    /// `import syntax._` bringing a `type HNil` alias into scope must not hide
    /// the top-level `object HNil` from `HNil.type`, so a scope that binds the
    /// name only in the type namespace is skipped and the search continues
    /// outward.
    pub fn lookup_term(&self, name: &str) -> Vec<SymbolId> {
        for sc in self.scopes.iter().rev() {
            let slot = sc.lookup_ranked(name);
            // The precedence the *term* binds at decides which entries of this
            // slot a term reference sees; the slot is handed back whole at
            // that rank, as this has always done.
            let Some(best) = slot
                .iter()
                .filter(|b| self.is_term_namespace(b.sym))
                .map(|b| b.rank)
                .min()
            else {
                continue;
            };
            return slot
                .iter()
                .filter(|b| b.rank == best)
                .map(|b| b.sym)
                .collect();
        }
        Vec::new()
    }

    /// Resolve a name the compiler synthesized for a class or object that nsc
    /// would have written as a fully qualified `scala.X` tree (see
    /// [`scala_rs_parser::Tree::scala_ref`]). It is a member lookup in package
    /// `scala`, not a lexical one, so no binding of that name anywhere in
    /// scope can capture it.
    ///
    /// The lexical fall-back matters in `--no-scala-library` mode, where the
    /// prelude enters some names into a scope of its own rather than into the
    /// package; it still skips a scope that binds the name only as a term, so
    /// `def Tuple2` never wins.
    pub fn lookup_scala(&self, name: &str) -> Vec<SymbolId> {
        let in_package: Vec<SymbolId> = self
            .lookup_member(self.scala_pkg, name)
            .into_iter()
            .filter(|&s| self.is_type_namespace(s) || self.is_module_like(s))
            .collect();
        if !in_package.is_empty() {
            return in_package;
        }
        for sc in self.scopes.iter().rev() {
            let found = best_ranked(sc.lookup_ranked(name), |s| {
                self.is_type_namespace(s) || self.is_module_like(s)
            });
            if !found.is_empty() {
                return found;
            }
        }
        Vec::new()
    }

    /// Look up the *function* of a constructor pattern (`case x :@ y`).
    ///
    /// nsc's `Context.lookupSymbol` qualifier for `typingConstructorPattern`
    /// drops `sym.isMethod && !sym.isStable`, so a plain `def` of that name
    /// never shadows an extractor further out. slick's `Node` declares
    /// `final def :@ (newType: Type): Self` and imports the extractor
    /// `object :@` from `TypeUtil`; without the rule, `val from2 :@ … = …`
    /// found the method and reported "not found: extractor :@".
    pub fn lookup_extractor(&self, name: &str) -> Vec<SymbolId> {
        for sc in self.scopes.iter().rev() {
            let found = best_ranked(sc.lookup_ranked(name), |s| {
                self.get(s).kind != SymKind::Method
            });
            if !found.is_empty() {
                return found;
            }
        }
        Vec::new()
    }

    /// Names that live in the term namespace under their own spelling.
    /// [`Self::is_term_namespace`] for callers outside this module.
    pub fn is_term_namespace_sym(&self, s: SymbolId) -> bool {
        self.is_term_namespace(s)
    }

    fn is_term_namespace(&self, s: SymbolId) -> bool {
        // The classfile loader uses one class symbol for a Java class and
        // its static companion. It therefore supplies both namespaces, unlike
        // a Scala class whose object has a separate module symbol. Shallow
        // binary Scala declarations also carry JAVA until completion; retaining
        // them lets final term resolution discover their actual companion.
        if self.get(s).kind == SymKind::Class && self.get(s).flags.contains(Flags::JAVA) {
            return true;
        }
        matches!(
            self.get(s).kind,
            SymKind::Term
                | SymKind::Method
                | SymKind::Module
                | SymKind::ModuleClass
                | SymKind::Package
        )
    }

    /// Names that live in the type namespace under their own spelling.
    fn is_type_namespace(&self, s: SymbolId) -> bool {
        matches!(
            self.get(s).kind,
            SymKind::Class | SymKind::TypeParam | SymKind::TypeMember
        )
    }

    fn is_module_like(&self, s: SymbolId) -> bool {
        matches!(self.get(s).kind, SymKind::Module | SymKind::ModuleClass)
    }

    /// Is `m`, declared in a *strict* ancestor of the class being looked up
    /// in, invisible from that class?
    ///
    /// `private` (and `private[this]`) members are not inherited: SLS 5.2.
    /// Without this, `trait A { private val x = 1 }; trait B { val x = 2 };
    /// trait C extends B with A { println(x) }` printed `1` -- whichever
    /// parent the traversal happened to reach first decided the answer, and
    /// the one that is not a member at all could win (`run/t7475b`).
    /// A qualified `private[C]` stays visible: the qualifier can name an
    /// enclosing package that does contain the subclass.
    pub(crate) fn private_to_owner(&self, m: SymbolId) -> bool {
        let s = self.get(m);
        s.flags.contains(Flags::PRIVATE) && s.private_within.is_none()
    }

    /// Every class a `self:` annotation makes visible from inside the class.
    ///
    /// A cake's self type is routinely a *compound*: gitbucket's
    /// `WikiControllerBase` writes
    /// `self: WikiService & RepositoryService & AccountService & …`, six
    /// components. `class_sym_of` answers a `Type::Refined` with its **first**
    /// parent alone, so only `WikiService`'s members were reachable
    /// unqualified and `getAccountByUserName$default$2` was "not a member of
    /// WikiControllerBase".
    pub fn self_type_classes(&self, ty: &Type) -> Vec<SymbolId> {
        match ty {
            Type::Refined { parents, .. } => parents
                .iter()
                .filter_map(|p| self.class_sym_of(p))
                .collect(),
            other => self.class_sym_of(other).into_iter().collect(),
        }
    }

    pub fn lookup_member(&self, owner: SymbolId, name: &str) -> Vec<SymbolId> {
        let mut out = Vec::new();
        let mut seen = rustc_hash::FxHashSet::default();
        let mut work = vec![owner];
        while let Some(id) = work.pop() {
            if !seen.insert(id.0) {
                continue;
            }
            let inherited = id != owner;
            let sym = self.get(id);
            for m in &sym.members {
                if self.get(*m).name == name && !(inherited && self.private_to_owner(*m)) {
                    out.push(*m);
                }
            }
            for m in &sym.parents {
                // `trait C[-T] extends (T => R)` really does inherit
                // `Function1.apply`; the parent just names no class until the
                // structural function is read back as one.
                let as_class = self.function_class_form(m);
                let m = as_class.as_ref().unwrap_or(m);
                if let Some(ps) = self.class_sym_of(m) {
                    work.push(ps);
                }
            }
            if let Some(st) = &sym.self_type {
                work.extend(self.self_type_classes(st));
            }
        }
        out
    }

    /// Every member `owner` has, including ones inherited from its parents --
    /// `lookup_member`'s traversal with no name to filter by.
    ///
    /// Used to fold a same-run package object's members into its package
    /// (see the `PACKAGE` case in `namer_module`): a package object need not
    /// declare a name itself to export it, and cats' `package object data
    /// extends ScalaVersionSpecificPackage` is exactly that --
    /// `NonEmptyLazyList` is a `type` alias declared on the parent class, not
    /// in the package object's own body, and folding only the object's direct
    /// members left the alias unreachable as `cats.data.NonEmptyLazyList`
    /// from any other file.
    pub fn members_including_inherited(&self, owner: SymbolId) -> Vec<SymbolId> {
        let mut out = Vec::new();
        let mut seen = rustc_hash::FxHashSet::default();
        let mut work = vec![owner];
        while let Some(id) = work.pop() {
            if !seen.insert(id.0) {
                continue;
            }
            let sym = self.get(id);
            out.extend(sym.members.iter().copied());
            for m in &sym.parents {
                let as_class = self.function_class_form(m);
                let m = as_class.as_ref().unwrap_or(m);
                if let Some(ps) = self.class_sym_of(m) {
                    work.push(ps);
                }
            }
            if let Some(st) = &sym.self_type {
                work.extend(self.self_type_classes(st));
            }
        }
        out
    }

    /// `lookup_member`, but walking only real `extends`/`with` parents, never
    /// a `self:` annotation. A self-type is a constraint on what a class may
    /// be *mixed into*, not a supertype: it makes the annotated type's own
    /// members visible from **inside** that class's body (which
    /// `lookup_member` models by also walking `self_type`, needed so
    /// `RelationalActionComponent { self: RelationalProfile => }` can call
    /// `RelationalProfile`'s members unqualified), but SLS 6.7.3 never lets
    /// `super.m` reach through it: `super` walks the actual mixin
    /// linearization only. Reusing `lookup_member` for `super.computeCapabilities`
    /// let `RelationalActionComponent`'s `self: RelationalProfile` answer for
    /// it via the self-type, which is `RelationalProfile`'s own
    /// still-being-completed override -- a false "recursive method
    /// computeCapabilities needs result type" instead of finding
    /// `BasicProfile`'s further up the real chain.
    pub fn lookup_member_real(&self, owner: SymbolId, name: &str) -> Vec<SymbolId> {
        let mut out = Vec::new();
        let mut seen = rustc_hash::FxHashSet::default();
        let mut work = vec![owner];
        while let Some(id) = work.pop() {
            if !seen.insert(id.0) {
                continue;
            }
            let sym = self.get(id);
            for m in &sym.members {
                if self.get(*m).name == name {
                    out.push(*m);
                }
            }
            for m in &sym.parents {
                let as_class = self.function_class_form(m);
                let m = as_class.as_ref().unwrap_or(m);
                if let Some(ps) = self.class_sym_of(m) {
                    work.push(ps);
                }
            }
        }
        out
    }

    /// nsc: a type parameter stands for its upper bound when its members are
    /// looked up (`def f[A <: Comparable[A]](x: A) = x.compareTo(...)`).
    /// Unbounded parameters are left alone so the caller still sees `A`.
    ///
    /// A *type constructor* parameter stands for its bound applied to the very
    /// arguments the application passes: `M[A]` where `M[+X] <: IterableOnce[X]`
    /// is an `IterableOnce[A]`. The bound is written in the constructor's own
    /// parameters, so it means nothing until they are replaced -- without this
    /// step `in.iterator` on an `M[A]` came back as `IterableOnce`'s own `A`
    /// and every use of the element was `found: A  required: A`.
    /// The tighter *lower* bound a method type parameter carries inside the
    /// `case` being typed (see `gadt_bounds`); innermost case wins.
    pub fn gadt_lo(&self, tp: SymbolId) -> Option<&Type> {
        self.gadt_bounds
            .iter()
            .rev()
            .find(|(id, _, _)| *id == tp)
            .and_then(|(_, lo, _)| lo.as_ref())
    }

    /// The tighter *upper* bound a method type parameter carries inside the
    /// `case` being typed (see `gadt_bounds`); innermost case wins.
    pub fn gadt_hi(&self, tp: SymbolId) -> Option<&Type> {
        self.gadt_bounds
            .iter()
            .rev()
            .find(|(id, _, _)| *id == tp)
            .and_then(|(_, _, hi)| hi.as_ref())
    }

    /// A type parameter the current case has bounded from above is, for
    /// member selection, that bound: `t + 1` on `t: T` inside `case I(_)`
    /// with `T` at `Int..Int` selects `Int.+`, as nsc reads members through
    /// the parameter's (temporarily tightened) `info.bounds.hi`.
    pub fn gadt_widen(&self, ty: &Type) -> Type {
        match ty {
            Type::TypeParam(id) => self.gadt_hi(*id).cloned().unwrap_or_else(|| ty.clone()),
            _ => ty.clone(),
        }
    }

    pub fn widen_type_param(&self, ty: &Type) -> Type {
        let mut t = ty.clone();
        // Following a bound that names the parameter it bounds (`A[X] <:
        // A[X]`) never terminates on its own, and the widened type it would
        // hand back still names `A`, so the caller loops instead. Stop at the
        // second visit and answer "nothing to widen to".
        let mut seen: Vec<u32> = Vec::new();
        loop {
            match &t {
                Type::BoundedWildcard { hi: Some(hi), .. } => t = *hi.clone(),
                Type::TypeParam(id) => {
                    if seen.contains(&id.0) {
                        return ty.clone();
                    }
                    seen.push(id.0);
                    match self.get(*id).bound_hi.clone() {
                        Some(hi) => t = hi,
                        None => return ty.clone(),
                    }
                }
                Type::Applied { ctor, args } => {
                    let Type::TypeParam(id) = ctor.as_ref() else {
                        break;
                    };
                    if seen.contains(&id.0) {
                        return ty.clone();
                    }
                    seen.push(id.0);
                    let tps = self.get(*id).tparams.clone();
                    let Some(hi) = self.get(*id).bound_hi.clone() else {
                        return ty.clone();
                    };
                    if tps.len() != args.len() {
                        return ty.clone();
                    }
                    t = subst_tparams_slice(&tps, args, &hi);
                }
                _ => break,
            }
        }
        if matches!(t, Type::TypeParam(_)) {
            return ty.clone();
        }
        t
    }

    /// What the singleton type `p.type` stands for, given `p`'s symbol.
    ///
    /// A `val` read from a pickle is installed as a **zero-argument method**:
    /// a class file cannot tell a `val`'s accessor from an ordinary `def`
    /// (see `Flags::ACCESSOR` in `pickle_supply::complete_named`). So
    /// `c.universe.type` names a symbol whose stored type is
    /// `Method { paramss: [], ret: Universe }`, and every reader of a
    /// `SingleType` that took `sym.ty` unwidened saw a shape it does not
    /// handle -- `class_sym_of` answered `None`, so the singleton conformed
    /// to nothing and erased to `Object`.
    pub fn singleton_underlying(&self, sym: SymbolId) -> Type {
        let symbol = self.get(sym);
        // Binary member modules may have only a module-class symbol, with
        // no companion term installed. Their singleton still widens to that
        // module class, not to the enclosing prefix's class.
        if symbol.is_class_like() && symbol.flags.contains(Flags::MODULE) {
            return Type::ModuleRef(sym);
        }
        match symbol.ty.clone() {
            Type::Method { paramss, ret } if paramss.iter().all(|c| c.is_empty()) => *ret,
            other => other,
        }
    }

    pub fn class_sym_of(&self, ty: &Type) -> Option<SymbolId> {
        match ty {
            Type::Class { sym, .. } | Type::ModuleRef(sym) => Some(*sym),
            Type::Int => Some(self.int_sym),
            Type::Byte => Some(self.byte_sym),
            Type::Short => Some(self.short_sym),
            Type::Long => Some(self.long_sym),
            Type::Float => Some(self.float_sym),
            Type::Double => Some(self.double_sym),
            Type::Char => Some(self.char_sym),
            Type::Boolean => Some(self.boolean_sym),
            Type::Unit => Some(self.unit_sym),
            Type::String => Some(self.string_sym),
            Type::Any => Some(self.any_sym),
            Type::AnyRef | Type::JavaObject => Some(self.anyref_sym),
            Type::AnyVal => Some(self.anyval_sym),
            Type::Array(_) => Some(self.array_sym),
            // `Null` is a subtype of every reference type; its members are
            // `AnyRef`'s.
            Type::Null => Some(self.anyref_sym),
            // A trait and its companion share a name; in type position the
            // class wins, so `object B extends B` does not become its own
            // parent.
            Type::Named { name, .. } => {
                let found = self.lookup_type(name);
                found
                    .iter()
                    .copied()
                    .find(|s| self.get(*s).kind == SymKind::Class)
                    .or_else(|| found.into_iter().find(|s| self.get(*s).is_class_like()))
            }
            // An unbounded type parameter's members are `Any`'s; a bounded one
            // resolves through its bound, as in nsc. `[A <: A]` and mutually
            // bounded parameters have no class to offer, so the chase stops
            // at `Any` rather than following the bound back to itself.
            Type::TypeParam(id) => match &self.get(*id).bound_hi {
                Some(hi) => {
                    let hi = hi.clone();
                    match enter_chase(Chase::ClassOf, *id) {
                        Some(_g) => self.class_sym_of(&hi),
                        None => Some(self.any_sym),
                    }
                }
                None => Some(self.any_sym),
            },
            Type::Applied { ctor, args } => {
                // A *fully applied* type lambda is the class its body names:
                // `([x, y] => (A0, x, y))[A0, P, Q]` is a `Tuple3`. Falling
                // straight through to the constructor reached the alias's
                // upper bound instead, so `fab.copy(_2 = x)` on cats'
                // `(A0, *, *)` found no case class to rebuild.
                if let Type::TypeMember(id) = ctor.as_ref() {
                    if self.get(*id).tparams.len() == args.len() {
                        let reduced = self.expand_applied_hk_alias(ty.clone());
                        if reduced != *ty {
                            return self.class_sym_of(&reduced);
                        }
                    }
                }
                self.class_sym_of(ctor)
            }
            Type::TypeMember(id) => {
                let Some(_g) = enter_chase(Chase::ClassOf, *id) else {
                    return Some(self.any_sym);
                };
                if !self.get(*id).tparams.is_empty() {
                    // A parameterized abstract member is not a class by itself,
                    // but `type C[T] <: TypedType[T]` still offers `TypedType`'s
                    // members, so member lookup goes through the upper bound.
                    // The chase is guarded: `type Self >: this.type <: Self`
                    // and mutually bounded members would otherwise loop.
                    let mut seen = rustc_hash::FxHashSet::default();
                    seen.insert(id.0);
                    self.bounded_member_class(*id, &mut seen)
                } else {
                    let seen = self.type_member_as_seen(*id);
                    if matches!(&seen, Type::TypeMember(x) if *x == *id) {
                        self.get(*id)
                            .bound_hi
                            .clone()
                            .as_ref()
                            .and_then(|h| self.class_sym_of(h))
                    } else {
                        self.class_sym_of(&seen)
                    }
                }
            }
            Type::BoundedWildcard { hi: Some(hi), .. } => self.class_sym_of(hi),
            Type::Wildcard | Type::BoundedWildcard { hi: None, .. } => Some(self.any_sym),
            Type::ThisType(sym) => Some(*sym),
            Type::Constant(lit) => self.class_sym_of(&Type::lit_underlying(lit)),
            Type::SingleType { prefix, sym } => {
                let Some(_g) = enter_chase(Chase::ClassOf, *sym) else {
                    return Some(self.any_sym);
                };
                let t = self.singleton_underlying(*sym);
                if t.is_no_type() {
                    self.class_sym_of(prefix)
                } else {
                    self.class_sym_of(&t)
                }
            }
            Type::Annotated { tpe, .. } => self.class_sym_of(tpe),
            Type::Refined { parents, .. } => parents
                .iter()
                .find_map(|p| self.class_sym_of(p))
                .or(Some(self.anyref_sym)),
            // `lookup_type`, not `lookup`: `TupleN` is a *type* name, and a
            // term of that name in a nearer scope must not answer for it.
            // `object Equiv` declares `implicit def Tuple2[T1, T2](…)`, so
            // inside its body plain `lookup` stopped at the method — the
            // innermost scope that binds the name at all — found nothing
            // class-like in it and gave up, and every `x._1` on a `(T1, T2)`
            // in `Ordering.scala` and `Equiv.scala` (176 of them) reported
            // "is not a member".
            Type::Tuple(ts) if !ts.is_empty() => self
                .lookup_type(&format!("Tuple{}", ts.len()))
                .into_iter()
                .find(|s| self.get(*s).is_class_like()),
            _ => None,
        }
    }

    /// Companion module of a class (same name, `SymKind::Module`, same owner).
    pub fn companion_module(&self, class_id: SymbolId) -> Option<SymbolId> {
        let s = self.get(class_id);
        if s.kind == SymKind::Module {
            return Some(class_id);
        }
        let name = s.name.clone();
        let owner = s.owner;
        self.get(owner)
            .members
            .iter()
            .copied()
            .find(|&m| self.get(m).kind == SymKind::Module && self.get(m).name == name)
    }

    /// The companion module *class* of `class_id`, for SLS 7.2's implicit
    /// scope, including the case where the two symbols do not share a name.
    ///
    /// A nested library class can reach the symbol table twice, by two routes
    /// that do not agree on either name or owner: flattened, from a JVM
    /// descriptor (`Printers$BooleanFlag`, owned by the package
    /// `scala.reflect.api`), and nested, from the enclosing class's pickle
    /// (`BooleanFlag$`, owned by the trait `Printers`). `companion_module`
    /// asks for the same name under the same owner and so joins neither to the
    /// other, and `scala.reflect.api.Printers.BooleanFlag` -- whose companion
    /// carries the only `Boolean => BooleanFlag` conversion there is -- had an
    /// empty implicit scope as a result.
    ///
    /// The JVM name is what joins them: the companion of `X` is `X$`. Only the
    /// implicit-scope callers use this; `companion_module`'s own answer is a
    /// *module* symbol, which a pickled module class is not.
    pub fn companion_module_class_for_implicits(&self, class_id: SymbolId) -> SymbolId {
        if let Some(m) = self.companion_module(class_id) {
            return self.module_class_of(m);
        }
        let jvm = &self.get(class_id).jvm_name;
        if jvm.is_empty() || jvm.ends_with('$') {
            return SymbolId::NONE;
        }
        let module = format!("{jvm}$");
        match self.find_class_by_jvm(&module) {
            Some(m) if self.get(m).kind == SymKind::ModuleClass => m,
            Some(m) if self.get(m).kind == SymKind::Module => self.module_class_of(m),
            _ => SymbolId::NONE,
        }
    }

    pub fn module_class_of(&self, id: SymbolId) -> SymbolId {
        match self.get(id).ty {
            Type::ModuleRef(c) => c,
            _ => id,
        }
    }

    /// Does this method take at least one value parameter?
    ///
    /// The question SLS 6.26.3 asks of an overloaded reference read in value
    /// position: the alternatives that take *no* parameters are the ones that
    /// survive there. An empty clause (`def f(): T`) counts as taking none,
    /// which is also how it behaves -- it is auto-applied.
    pub fn takes_value_params(&self, id: SymbolId) -> bool {
        match &self.get(id).ty {
            Type::Method { paramss, .. } => paramss.iter().any(|c| !c.is_empty()),
            _ => false,
        }
    }

    /// Substitute `args` for the type parameters `tps`, and nothing else: no
    /// projection reduction, no beta-reduction of the result. The backend's
    /// pickler needs exactly this when it closes a type lambda over the
    /// parameters the typer left partially applied (`pickle::pickle_lambda_alias`).
    pub fn subst_type_params(&self, tps: &[SymbolId], args: &[Type], ty: &Type) -> Type {
        if tps.is_empty() || args.is_empty() {
            return ty.clone();
        }
        subst_map(ty, tps, args)
    }

    /// Substitute class type arguments into a member type (`List[Int].head` → `Int`).
    pub fn subst_tparams(&self, owner: SymbolId, args: &[Type], ty: &Type) -> Type {
        // Borrowed, not cloned: the common call has no type parameters at all
        // and returns on the next line, and this is one of the hottest
        // functions in the typer.
        let tps = &self.get(owner).tparams;
        if tps.is_empty() || args.is_empty() {
            return ty.clone();
        }
        let out = subst_map(ty, tps, args);
        // `P#T` reduces exactly here: `P` is one of `tps` and `args` is what
        // it has just become. See `subst_projections`.
        let out = self.subst_projections(tps, args, &out);
        // Substituting a type *lambda* for a type constructor leaves the
        // applications it lands in folded: `def twice[F[_]](fa: F[Int])` with
        // `F = ({ type L[X] = Reader[Int, X] })#L` gives `L[Int]`, and
        // `subst_map` is a free function that cannot reach the body. Reduce
        // them here. The guard costs one match per argument and fails at once
        // for every ordinary type.
        if args.iter().any(|a| self.hk_alias(a).is_some()) {
            return self.expand_hk_aliases(&out);
        }
        out
    }

    /// Beta-reduce every fully applied higher-kinded alias inside `ty`.
    pub fn expand_hk_aliases(&self, ty: &Type) -> Type {
        let go = |t: &Type| self.expand_hk_aliases(t);
        match ty {
            Type::Applied { ctor, args } => {
                let applied = apply_type_ctor(go(ctor), args.iter().map(go).collect());
                self.expand_applied_hk_alias(applied)
            }
            Type::Class { sym, args } => Type::Class {
                sym: *sym,
                args: args.iter().map(go).collect(),
            },
            Type::Tuple(ts) => Type::Tuple(ts.iter().map(go).collect()),
            Type::Array(t) => Type::Array(Box::new(go(t))),
            Type::ByName(t) => Type::ByName(Box::new(go(t))),
            Type::Repeated(t) => Type::Repeated(Box::new(go(t))),
            Type::Function { params, ret } => Type::Function {
                params: params.iter().map(go).collect(),
                ret: Box::new(go(ret)),
            },
            Type::Method { paramss, ret } => Type::Method {
                paramss: paramss
                    .iter()
                    .map(|ps| ps.iter().map(go).collect())
                    .collect(),
                ret: Box::new(go(ret)),
            },
            Type::Annotated { tpe, annot } => Type::Annotated {
                tpe: Box::new(go(tpe)),
                annot: annot.clone(),
            },
            Type::Refined { parents, decls } => Type::Refined {
                parents: parents.iter().map(go).collect(),
                decls: decls
                    .iter()
                    .map(|d| expand_hk_refine_decl(self, d))
                    .collect(),
            },
            other => other.clone(),
        }
    }

    /// [`SymbolTable::subst_tparams`] without the copy when the substitution
    /// is the identity. See [`subst_tparams_cow`].
    pub(crate) fn subst_tparams_cow<'t>(
        &self,
        owner: SymbolId,
        args: &[Type],
        ty: &'t Type,
    ) -> std::borrow::Cow<'t, Type> {
        let tps = &self.get(owner).tparams;
        let out = subst_tparams_cow(tps, args, ty);
        if self.abs_projection_of.is_empty() {
            return out;
        }
        match self.subst_projections(tps, args, &out) {
            t if t == *out => out,
            t => std::borrow::Cow::Owned(t),
        }
    }

    /// nsc: a *alias* type member is equivalent to (not merely bounded by) its
    /// right-hand side, so `type Scope = Map[K, V]` and `Map[K, V]` are the same
    /// type in both directions. Abstract members (`type T <: Bound`) have no
    /// right-hand side and are left alone; so are higher-kinded aliases, which
    /// only expand once applied (`expand_applied_hk_alias`). The walk is bounded
    /// because a pickled or malformed chain can be cyclic.
    pub fn dealias(&self, ty: &Type) -> Type {
        let mut t = ty.clone();
        let mut seen: Vec<u32> = Vec::new();
        while let Type::TypeMember(id) = &t {
            if seen.contains(&id.0) {
                return ty.clone();
            }
            seen.push(id.0);
            let next = self.type_member_as_seen(*id);
            if next == t {
                break;
            }
            t = next;
        }
        t
    }

    /// Use-site view of a type member: keep higher-kinded aliases as constructors
    /// (`type F[X] = Id[X]` stays `TypeMember` until applied as `F[Int]`).
    /// Is `id` a type member the program left *deferred* (`type A`, `type A <:
    /// B`), as opposed to one it fixed with a right-hand side (`type A = B`)?
    ///
    /// A `TypeMember` symbol carries its right-hand side in `ty`; a deferred
    /// one has none, which is spelled either as `NoType` or as a self
    /// reference. nsc's rule is that a concrete definition overrides a
    /// deferred one, so every place that picks one member out of a class's
    /// linearisation has to be able to tell them apart.
    pub fn is_deferred_type_member(&self, id: SymbolId) -> bool {
        let info = self.get(id);
        if info.kind != SymKind::TypeMember {
            return false;
        }
        match &info.ty {
            Type::NoType | Type::Error => true,
            Type::TypeMember(inner) => *inner == id,
            _ => false,
        }
    }

    pub fn type_member_as_seen(&self, id: SymbolId) -> Type {
        if !self.get(id).tparams.is_empty() {
            Type::TypeMember(id)
        } else {
            match self.get(id).ty.clone() {
                Type::NoType | Type::Error => Type::TypeMember(id),
                // A *deferred* member stands for itself; an alias whose
                // right-hand side is some *other* member is an alias like any
                // other. `new FlatMap[B] { type Start = c.Start }` is the
                // second kind, and folding it to itself left the anonymous
                // class's `Start` opaque, so `c.start`'s `c.Start` never met
                // it (cats' `Eval#flatMap`). This is exactly the distinction
                // `is_deferred_type_member` draws.
                Type::TypeMember(inner) if inner == id => Type::TypeMember(id),
                other => other,
            }
        }
    }

    /// The declaration `id` stands for when `id` is a path-dependent member
    /// (`p.T` -> `T`), and `None` for every other symbol.
    pub fn path_member_decl(&self, id: SymbolId) -> Option<SymbolId> {
        self.path_member_decl.get(&id).copied()
    }

    /// The path `id` was projected out of, for a path-dependent member.
    pub fn path_member_path(&self, id: SymbolId) -> Option<&[SymbolId]> {
        self.path_member_path.get(&id).map(|v| v.as_slice())
    }

    /// `p.T` as a type, given the term path `p` and the declaration `T`.
    ///
    /// The symbol is allocated once per pair and reused, so two spellings of
    /// the same path give the same type and compare equal; two different paths
    /// give two symbols and do not. `prefix` is the path's own type, used to
    /// read the declaration's bounds through it (`type T <: List[A]` of an
    /// `A[Int]` is bounded by `List[Int]` here).
    pub fn path_member(&mut self, path: &[SymbolId], decl: SymbolId, prefix: &Type) -> SymbolId {
        let key = (path.to_vec(), decl);
        if let Some(id) = self.path_members.get(&key) {
            return *id;
        }
        let info = self.get(decl);
        let name = info.name.clone();
        let flags = info.flags;
        let tparams = info.tparams.clone();
        let lo = info.bound_lo.clone();
        let hi = info.bound_hi.clone();
        let owner = *path.last().unwrap_or(&SymbolId::NONE);
        let id = self.alloc(name, owner, SymKind::TypeMember, flags, "");
        // A deferred member stands for itself: `p.T` is abstract exactly when
        // `T` is, and a concrete `T` never reaches here (the alias expands and
        // carries the prefix along in its right-hand side).
        self.note_mutation();
        self.symbols[id.0 as usize].ty = Type::TypeMember(id);
        self.symbols[id.0 as usize].tparams = tparams;
        self.symbols[id.0 as usize].bound_lo = lo.map(|t| self.expand_in_type(prefix, &t));
        self.symbols[id.0 as usize].bound_hi = hi.map(|t| self.expand_in_type(prefix, &t));
        self.path_members.insert(key, id);
        self.path_member_decl.insert(id, decl);
        self.path_member_path.insert(id, path.to_vec());
        id
    }

    /// `P#T` as a type, where the prefix `P` is a type parameter or a type
    /// member the program has left deferred, so nothing here can reduce it.
    ///
    /// Allocated once per (prefix, declaration) pair and reused, so two
    /// spellings of the same projection are the same type. The bounds are
    /// the declaration's own, minus any that mention the declaring class's
    /// type parameters: those are written in a vocabulary the projection has
    /// no arguments for, and a bound that means nothing here would make
    /// unrelated types conform.
    pub fn abstract_projection(&mut self, prefix: SymbolId, decl: SymbolId) -> SymbolId {
        if let Some(id) = self.abs_projections.get(&(prefix, decl)) {
            return *id;
        }
        let info = self.get(decl);
        let name = info.name.clone();
        let flags = info.flags;
        let tparams = info.tparams.clone();
        let owner_tps = self.get(info.owner).tparams.clone();
        let keep = |st: &Self, t: Option<Type>| -> Option<Type> {
            let t = t?;
            (!st.mentions_tparams(&t, &owner_tps)).then_some(t)
        };
        let lo = keep(self, info.bound_lo.clone());
        let hi = keep(self, info.bound_hi.clone());
        let id = self.alloc(name, prefix, SymKind::TypeMember, flags, "");
        // Deferred, and it stands for itself: an abstract projection is
        // exactly as opaque as the declaration it projects until the prefix
        // is instantiated.
        self.note_mutation();
        self.symbols[id.0 as usize].ty = Type::TypeMember(id);
        self.symbols[id.0 as usize].tparams = tparams;
        self.symbols[id.0 as usize].bound_lo = lo;
        self.symbols[id.0 as usize].bound_hi = hi;
        self.abs_projections.insert((prefix, decl), id);
        self.abs_projection_of.insert(id, (prefix, decl));
        id
    }

    fn mentions_tparams(&self, ty: &Type, tps: &[SymbolId]) -> bool {
        if tps.is_empty() {
            return false;
        }
        any_type(
            ty,
            &mut |t| matches!(t, Type::TypeParam(id) | Type::TypeMember(id) if tps.contains(id)),
        )
    }

    /// `(prefix, declaration)` when `id` is an abstract projection, `None`
    /// otherwise.
    pub fn abs_projection(&self, id: SymbolId) -> Option<(SymbolId, SymbolId)> {
        self.abs_projection_of.get(&id).copied()
    }

    /// The declaration `id` stands for when it is a path-dependent member or
    /// an abstract projection: what erasure, the pickle and the backend see,
    /// so that nothing downstream of the typer has to know either exists.
    pub fn projected_decl(&self, id: SymbolId) -> Option<SymbolId> {
        self.path_member_decl
            .get(&id)
            .copied()
            .or_else(|| self.abs_projection_of.get(&id).map(|(_, d)| *d))
    }

    /// `pre#T` reduced, or `None` when `pre` does not settle `T`.
    ///
    /// `None` covers the two cases that must *not* reduce: a prefix that is
    /// still abstract (real scalac rejects `class TQ[E <: Table[_]] extends
    /// Query[E, E#Element]` precisely because `E#Element` is not the bound's
    /// `Element`), and a class that inherits the declaration without fixing
    /// it.
    pub fn reduce_projection(&self, pre: &Type, decl: SymbolId) -> Option<Type> {
        // A prefix that is still abstract settles nothing, and reading it
        // through its *bound* is the too-eager reduction: `class TQ[E <:
        // Table[_]] extends Query[E, E#Element]` is rejected by real scalac
        // exactly because `E#Element` is not the bound's `Element`.
        // `class_sym_of` would follow the bound, so it is ruled out first.
        if matches!(pre, Type::TypeParam(_))
            || matches!(pre, Type::TypeMember(id) if self.is_deferred_type_member(*id))
        {
            return None;
        }
        let name = self.get(decl).name.clone();
        let cls = self.class_sym_of(pre)?;
        let owner = self.get(decl).owner;
        // Only a class that really has this declaration above it can answer
        // for it; anything else would be a different member of the same name.
        if owner != cls && !self.is_ancestor_of(owner, cls) {
            return None;
        }
        let m = self
            .type_members_named(cls, &name)
            .into_iter()
            .find(|m| self.get(*m).kind == SymKind::TypeMember)?;
        if self.is_deferred_type_member(m) || !self.get(m).tparams.is_empty() {
            return None;
        }
        let rhs = self.type_member_as_seen(m);
        Some(self.subst_as_seen_from(pre, &rhs))
    }

    /// Does `ty` mention an abstract projection anywhere?
    pub fn mentions_abs_projection(&self, ty: &Type) -> bool {
        if self.abs_projection_of.is_empty() {
            return false;
        }
        any_type(
            ty,
            &mut |t| matches!(t, Type::TypeMember(id) if self.abs_projection_of.contains_key(id)),
        )
    }

    /// Replace every abstract projection in `ty` by the declaration it
    /// projects. Erasure, the pickle and the backend read types through this,
    /// so nothing downstream of the typer has to know projections exist.
    pub fn drop_abs_projections(&self, ty: &Type) -> Type {
        if self.abs_projection_of.is_empty() {
            return ty.clone();
        }
        map_type(ty, &mut |t| match t {
            Type::TypeMember(id) => self
                .abs_projection_of
                .get(id)
                .map(|(_, d)| Type::TypeMember(*d))
                .unwrap_or_else(|| t.clone()),
            other => other.clone(),
        })
    }

    /// Reduce every `P#T` in `ty` whose prefix `P` is one of `tps`, now that
    /// `args` says what `P` is.
    ///
    /// This is the whole point of the representation: `TableQuery[E <:
    /// AbstractTable[_]] extends Query[E, E#TableElementType, Seq]` is read
    /// out of slick's pickle with `E#TableElementType` unreduced, and the
    /// reduction fires here -- once, at `E := Accounts`, where `Accounts`
    /// really does fix `TableElementType` to `(String, Int)`.
    ///
    /// A projection whose new prefix settles nothing -- `E'#T` at `E' := E`,
    /// another abstract type -- becomes the bare **declaration**, not a
    /// projection through the new prefix. Rebuilding one would need `&mut
    /// self` on one of the hottest paths in the typer; keeping the old one
    /// would leave a projection through a prefix that is no longer in scope,
    /// and two such stale symbols do not compare equal to each other
    /// (`def widenOf[E <: AbstractRow](rs: RowSet[E]): E#ElementType =
    /// rs.widen` reported `found: E#ElementType required: E#ElementType`).
    /// The bare declaration is exactly the answer this compiler produced
    /// everywhere before projections existed, and `is_sub_type` relates it to
    /// a projection in both directions, so no question gets a worse answer
    /// than it had.
    pub fn subst_projections(&self, tps: &[SymbolId], args: &[Type], ty: &Type) -> Type {
        if !self.mentions_abs_projection(ty) {
            return ty.clone();
        }
        map_type(ty, &mut |t| {
            let Type::TypeMember(id) = t else {
                return t.clone();
            };
            let Some((pre, decl)) = self.abs_projection(*id) else {
                return t.clone();
            };
            let Some(i) = tps.iter().position(|p| *p == pre) else {
                return t.clone();
            };
            let Some(arg) = args.get(i) else {
                return t.clone();
            };
            self.reduce_projection(arg, decl)
                .unwrap_or(Type::TypeMember(decl))
        })
    }

    /// Every type member `ty` mentions anywhere, refinements included.
    pub fn type_members_in(&self, ty: &Type) -> Vec<SymbolId> {
        let mut out: Vec<SymbolId> = Vec::new();
        any_type(ty, &mut |t| {
            if let Type::TypeMember(id) = t {
                if !out.contains(id) {
                    out.push(*id);
                }
            }
            false
        });
        out
    }

    /// Does `ty` itself spell a path-dependent member anywhere?
    ///
    /// Shallow on purpose. `is_sub_type` pairs this with `drop_path_members`,
    /// which cannot look inside a type lambda either -- it takes `&self` and a
    /// rewritten lambda is a new symbol. Answering "yes" for a member this
    /// walk's partner cannot then remove is an `is_sub_type` that recurses on
    /// an unchanged type until the stack runs out.
    pub fn mentions_path_member(&self, ty: &Type) -> bool {
        if self.path_member_decl.is_empty() {
            return false;
        }
        any_type(
            ty,
            &mut |t| matches!(t, Type::TypeMember(id) if self.path_member_decl.contains_key(id)),
        )
    }

    /// Does `ty` mention a path-dependent member anywhere, the bodies of the
    /// type lambdas it reaches included?
    ///
    /// A type lambda is a symbol whose *body* is stored beside it, so a walk
    /// that means to find every occurrence has to look inside one:
    /// `Parallel.Aux[EitherT[M, E, *], Nested[P.F, Validated[E, *], *]]` keeps
    /// `P.F` in the body of the anonymous alias kind-projector's `*` produced,
    /// and nowhere in the type itself.
    pub fn mentions_path_member_deep(&self, ty: &Type) -> bool {
        if self.path_member_decl.is_empty() {
            return false;
        }
        let mut seen: Vec<SymbolId> = Vec::new();
        self.mentions_path_member_in(ty, &mut seen)
    }

    fn mentions_path_member_in(&self, ty: &Type, seen: &mut Vec<SymbolId>) -> bool {
        let mut inner: Vec<SymbolId> = Vec::new();
        let hit = any_type(ty, &mut |t| match t {
            Type::TypeMember(id) if self.path_member_decl.contains_key(id) => true,
            Type::TypeMember(id) => {
                if self.is_structural_alias(*id) && !seen.contains(id) && !inner.contains(id) {
                    inner.push(*id);
                }
                false
            }
            _ => false,
        });
        if hit {
            return true;
        }
        for id in inner {
            seen.push(id);
            let body = self.get(id).ty.clone();
            if self.mentions_path_member_in(&body, seen) {
                return true;
            }
        }
        false
    }

    /// Every path-dependent member `ty` mentions, the bodies of the anonymous
    /// type-lambda aliases it reaches included. `type_members_in` sees only
    /// what the type itself spells, which for a lambda is the alias symbol.
    pub fn path_members_in(&self, ty: &Type) -> Vec<SymbolId> {
        let mut out: Vec<SymbolId> = Vec::new();
        let mut seen: Vec<SymbolId> = Vec::new();
        self.path_members_in_into(ty, &mut out, &mut seen);
        out
    }

    fn path_members_in_into(&self, ty: &Type, out: &mut Vec<SymbolId>, seen: &mut Vec<SymbolId>) {
        let mut inner: Vec<SymbolId> = Vec::new();
        any_type(ty, &mut |t| {
            if let Type::TypeMember(id) = t {
                if self.path_member_decl.contains_key(id) {
                    if !out.contains(id) {
                        out.push(*id);
                    }
                } else if self.is_structural_alias(*id) && !seen.contains(id) {
                    seen.push(*id);
                    inner.push(*id);
                }
            }
            false
        });
        for id in inner {
            let body = self.get(id).ty.clone();
            self.path_members_in_into(&body, out, seen);
        }
    }

    /// An anonymous alias standing for a structural type member -- what a type
    /// lambda (`[a]Either[String, a]`, kind-projector's `Either[String, *]`)
    /// and a refinement's `type L = …` are both represented as. Its body lives
    /// in the symbol table rather than in the type, so a substitution that
    /// means to reach every occurrence has to go through it, and a rewrite has
    /// to allocate a fresh alias rather than edit this one in place: the same
    /// symbol stands for every use of that written type.
    ///
    /// Only an *anonymous* one. A class's own `type X = …` is a member other
    /// code resolves by name, and cloning it would make two members where the
    /// program declares one.
    fn is_structural_alias(&self, id: SymbolId) -> bool {
        let s = self.get(id);
        s.owner.is_none()
            && s.is_type_alias
            && s.kind == SymKind::TypeMember
            && !matches!(&s.ty, Type::TypeMember(x) if *x == id)
    }

    /// Replace the path-dependent member `from` by `to` throughout `ty`,
    /// reaching into the body of every anonymous type-lambda alias on the way
    /// by allocating a rewritten copy of it.
    ///
    /// cats' `def catsDataParallelForEitherTWithParallelEffect[M[_], E:
    /// Semigroup](implicit P: Parallel[M]): Parallel.Aux[EitherT[M, E, *],
    /// Nested[P.F, Validated[E, *], *]] = accumulatingParallel[M, E]` is the
    /// case: the callee's result names the callee's own `P`, the caller has a
    /// `P` of its own, and every occurrence to substitute sits inside a lambda.
    pub fn subst_path_member_deep(&mut self, ty: &Type, from: SymbolId, to: &Type) -> Type {
        if !self.mentions_path_member_deep(ty) {
            return ty.clone();
        }
        let mut stack: Vec<SymbolId> = Vec::new();
        self.subst_path_member_in(ty, from, to, &mut stack)
    }

    fn subst_path_member_in(
        &mut self,
        ty: &Type,
        from: SymbolId,
        to: &Type,
        stack: &mut Vec<SymbolId>,
    ) -> Type {
        // Which anonymous aliases in `ty` have to be rewritten, and to what.
        let mut aliases: Vec<SymbolId> = Vec::new();
        any_type(ty, &mut |t| {
            if let Type::TypeMember(id) = t {
                if self.is_structural_alias(*id) && !aliases.contains(id) {
                    aliases.push(*id);
                }
            }
            false
        });
        let mut rewritten: Vec<(SymbolId, SymbolId)> = Vec::new();
        for a in aliases {
            // A recursive alias would otherwise clone itself forever.
            if stack.contains(&a) {
                continue;
            }
            let body = self.get(a).ty.clone();
            let mut probe = stack.clone();
            probe.push(a);
            if !self.mentions_path_member_in(&body, &mut probe) {
                continue;
            }
            if let Some(id) = self
                .path_member_lambdas
                .get(&(a, from))
                .and_then(|v| v.iter().find(|(t, _)| t == to))
                .map(|(_, id)| *id)
            {
                rewritten.push((a, id));
                continue;
            }
            let info = self.get(a);
            let (name, flags, tparams, lo, hi) = (
                info.name.clone(),
                info.flags,
                info.tparams.clone(),
                info.bound_lo.clone(),
                info.bound_hi.clone(),
            );
            let clone = self.alloc(&name, SymbolId::NONE, SymKind::TypeMember, flags, "");
            self.note_mutation();
            self.symbols[clone.0 as usize].tparams = tparams;
            self.symbols[clone.0 as usize].is_type_alias = true;
            // Insert before recursing: a body that reaches this alias again
            // then finds the copy instead of starting a second one.
            self.path_member_lambdas
                .entry((a, from))
                .or_default()
                .push((to.clone(), clone));
            stack.push(a);
            let new_body = self.subst_path_member_in(&body, from, to, stack);
            let lo = lo.map(|t| self.subst_path_member_in(&t, from, to, stack));
            let hi = hi.map(|t| self.subst_path_member_in(&t, from, to, stack));
            stack.pop();
            self.note_mutation();
            self.symbols[clone.0 as usize].ty = new_body;
            self.symbols[clone.0 as usize].bound_lo = lo;
            self.symbols[clone.0 as usize].bound_hi = hi;
            rewritten.push((a, clone));
        }
        map_type(ty, &mut |t| match t {
            Type::TypeMember(id) if *id == from => to.clone(),
            Type::TypeMember(id) => match rewritten.iter().find(|(a, _)| a == id) {
                Some((_, c)) => Type::TypeMember(*c),
                None => t.clone(),
            },
            other => other.clone(),
        })
    }

    /// Replace every path-dependent member in `ty` by the declaration it
    /// stands for. Erasure, the pickle and the backend read types through
    /// this, so nothing downstream of the typer has to know that paths exist.
    pub fn drop_path_members(&self, ty: &Type) -> Type {
        if self.path_member_decl.is_empty() {
            return ty.clone();
        }
        map_type(ty, &mut |t| match t {
            Type::TypeMember(id) => self
                .path_member_decl
                .get(id)
                .map(|d| Type::TypeMember(*d))
                .unwrap_or_else(|| t.clone()),
            other => other.clone(),
        })
    }

    /// The class that supplies the members of a parameterized abstract type
    /// member, found by following its upper bound. `type B[T] <: C[T]` and
    /// `type C[T] <: TypedType[T]` together make `TypedType` the answer for `B`.
    /// `seen` stops the walk on recursive or mutually bounded members.
    fn bounded_member_class(
        &self,
        id: SymbolId,
        seen: &mut rustc_hash::FxHashSet<u32>,
    ) -> Option<SymbolId> {
        fn head(
            st: &SymbolTable,
            ty: &Type,
            seen: &mut rustc_hash::FxHashSet<u32>,
        ) -> Option<SymbolId> {
            match ty {
                Type::Class { sym, .. } => Some(*sym),
                Type::Applied { ctor, .. } => head(st, ctor, seen),
                Type::Annotated { tpe, .. } => head(st, tpe, seen),
                Type::Refined { parents, .. } => parents.iter().find_map(|p| head(st, p, seen)),
                Type::TypeMember(inner) => {
                    if !seen.insert(inner.0) {
                        return None;
                    }
                    st.bounded_member_class(*inner, seen)
                }
                _ => None,
            }
        }
        let info = self.get(id);
        if !matches!(&info.ty, Type::NoType | Type::Error | Type::TypeMember(_)) {
            let rhs = info.ty.clone();
            if let Some(c) = head(self, &rhs, seen) {
                return Some(c);
            }
        }
        let hi = info.bound_hi.clone()?;
        head(self, &hi, seen)
    }

    /// `F[Int]` where `type F[X] = Id[X]` → `Id[Int]`. Abstract `type F[_]` stays applied.
    pub fn expand_applied_hk_alias(&self, ty: Type) -> Type {
        match &ty {
            Type::Applied { ctor, args } => {
                if let Type::TypeMember(id) = ctor.as_ref() {
                    let info = self.get(*id);
                    if !info.tparams.is_empty()
                        && info.tparams.len() == args.len()
                        && !matches!(&info.ty, Type::NoType | Type::Error | Type::TypeMember(_))
                    {
                        // A higher-kinded alias whose body applies the alias
                        // again (`type a[s[_], z] = s[n#a[s, z]]`, the
                        // Peano-arithmetic shape of `pos/t2994a`) grows one
                        // layer per expansion. Expanding it once is what
                        // callers want; expanding it inside its own expansion
                        // never ends, so leave the inner one folded.
                        if let Some(_g) = enter_chase(Chase::HkAlias, *id) {
                            return self.subst_tparams(*id, args, &info.ty);
                        }
                    }
                }
                ty
            }
            _ => ty,
        }
    }

    /// The parameters and right-hand side of a higher-kinded *alias* type
    /// member -- that is, a type lambda.
    ///
    /// `type L[a] = Either[String, a]` is one whether it is written as a named
    /// alias or inside a refinement (`({ type L[a] = Either[String, a] })#L`).
    /// An *abstract* higher-kinded member (`type F[_]`, whose stored type is
    /// the placeholder `TypeMember(self)`) is not a lambda and answers `None`.
    /// A lambda that captures an enclosing type parameter is handed out
    /// partially applied (`refinement_type_member`), so `Applied` counts too;
    /// the parameters still to come are the ones the arguments have not eaten.
    fn hk_alias(&self, ty: &Type) -> Option<(&[SymbolId], &Type)> {
        let (id, applied) = match ty {
            Type::TypeMember(id) => (*id, 0),
            Type::Applied { ctor, args } => match ctor.as_ref() {
                Type::TypeMember(id) => (*id, args.len()),
                _ => return None,
            },
            _ => return None,
        };
        let info = self.get(id);
        if info.tparams.len() <= applied {
            return None;
        }
        match &info.ty {
            Type::NoType | Type::Error | Type::TypeMember(_) => None,
            body => Some((&info.tparams[applied..], body)),
        }
    }

    /// Apply two type constructors of the same arity to one set of parameters,
    /// so that their bodies can be compared, when at least one of them is a
    /// type lambda.
    ///
    /// nsc holds `({ type L[a] = Either[String, a] })#L`, a named
    /// `type EitherL[a] = Either[String, a]`, and a second written copy of the
    /// same refinement to be one type: it dealiases all of them to the same
    /// lambda. Here every written refinement allocates its own `TypeMember`
    /// symbol, so the symbols can never match and `dealias` will not unfold a
    /// higher-kinded alias (its body is only meaningful once applied). Apply
    /// both sides to one side's own parameters and compare the results.
    ///
    /// `None` -- meaning "the caller decides" -- unless both sides are things
    /// eta-expansion actually says something about: a type lambda, or a class
    /// constructor with arguments still to come (`Fun[List]` conforms to
    /// `Fun[({ type L[a] = List[a] })#L]`). Abstract members and higher-kinded
    /// type *parameters* are deliberately left to the arms below.
    pub(crate) fn eta_expand_pair(&self, a: &Type, b: &Type) -> Option<(Type, Type)> {
        let n = self.kind_arity(a);
        if n == 0 || self.kind_arity(b) != n {
            return None;
        }
        let eta_ok = |t: &Type| {
            self.hk_alias(t).is_some() || matches!(crate::prefix::strip_view(t), Type::Class { .. })
        };
        if !eta_ok(a) || !eta_ok(b) {
            return None;
        }
        let (params, _) = self.hk_alias(a).or_else(|| self.hk_alias(b))?;
        if params.len() != n {
            return None;
        }
        let args: Vec<Type> = params.iter().map(|p| Type::TypeParam(*p)).collect();
        let ea = self.expand_applied_hk_alias(apply_type_ctor(a.clone(), args.clone()));
        let eb = self.expand_applied_hk_alias(apply_type_ctor(b.clone(), args));
        Some((ea, eb))
    }

    /// Eta-expand a pair where one side is a type lambda and the other is an
    /// *abstract* constructor -- a higher-kinded type parameter (`CC[_]`) or an
    /// abstract type member (`type F[_]`), the two shapes
    /// [`SymbolTable::eta_expand_pair`] deliberately declines.
    ///
    /// nsc has no such split. `isHKSubType` normalizes both sides, which
    /// eta-expands each to a `PolyType`, and `isPolySubType` then asks
    /// `sameLength` on the parameters and compares the bodies -- for an
    /// abstract constructor exactly as for a class or an alias. Normalizing an
    /// `AliasTypeRef` also beta-reduces its body, so a lambda whose body does
    /// not mention its parameter reduces to that body: `type AnyConstr[X] =
    /// Any` is `[X]Any`, and `[x]CC[x] <:< [X]Any` holds for *every* `CC`
    /// because `CC[x] <:< Any` does. That is `scala.collection`'s own
    /// `AnyConstr`, and it is why `IterableOps[A, CC, C]` is an
    /// `IterableOps[A, AnyConstr, _]`.
    ///
    /// Requiring one side to be a lambda is the point: this reduces an *alias*,
    /// it does not relate two abstract constructors to each other. The kinds of
    /// the parameters have to agree as well as their number -- `sameLength` is
    /// nsc's arity check, and `cmp` over `corresponds` is its kind check.
    fn eta_expand_abstract_pair(&self, a: &Type, b: &Type) -> Option<(Type, Type)> {
        let n = self.kind_arity(a);
        if n == 0 || self.kind_arity(b) != n {
            return None;
        }
        let abstract_ctor = |t: &Type| {
            let head = match t {
                Type::Applied { ctor, .. } => ctor.as_ref(),
                other => other,
            };
            matches!(head, Type::TypeParam(_) | Type::TypeMember(_))
        };
        // Exactly one side is the lambda being reduced; the other is abstract.
        let (params, _) = match (self.hk_alias(a), self.hk_alias(b)) {
            (Some(la), None) if abstract_ctor(b) => la,
            (None, Some(lb)) if abstract_ctor(a) => lb,
            _ => return None,
        };
        if params.len() != n || self.tparam_arities(a) != self.tparam_arities(b) {
            return None;
        }
        let args: Vec<Type> = params.iter().map(|p| Type::TypeParam(*p)).collect();
        let ea = self.expand_applied_hk_alias(apply_type_ctor(a.clone(), args.clone()));
        let eb = self.expand_applied_hk_alias(apply_type_ctor(b.clone(), args));
        Some((ea, eb))
    }

    /// Conformance between two type constructors, decided on their bodies.
    /// See [`SymbolTable::eta_expand_pair`].
    fn hk_alias_sub_type(&self, a: &Type, b: &Type) -> Option<bool> {
        if let Some((ea, eb)) = self.eta_expand_pair(a, b) {
            return Some(self.is_sub_type(&ea, &eb));
        }
        // The abstract half is deliberately *not* conclusive. Where both sides
        // eta-expand, their bodies are the whole story and a `false` is an
        // answer. An abstract constructor's body is `CC[x]` and says nothing on
        // its own, so the arms below -- bounds, path members, projections --
        // still have to run. This arm can only add acceptances, which is what
        // keeps `type G[X] = List[X]` from making `Option` conform to `G`:
        // `Option[x] <: List[x]` is false here and false there.
        let (ea, eb) = self.eta_expand_abstract_pair(a, b)?;
        if self.is_sub_type(&ea, &eb) {
            Some(true)
        } else {
            None
        }
    }

    /// Does the type *constructor* `ctor` satisfy the proper-type bound
    /// `bound` (`def f[F[_, _] <: Product]` instantiated at a type lambda)?
    ///
    /// nsc keeps a higher-kinded parameter's bounds *inside* its `PolyType`,
    /// so `F[_, _] <: Product` is really `[x, y]F[x, y] <: [x, y]Product` and
    /// `isPolySubType` decides it on the bodies. Here the bound is stored as
    /// the written `Product`, so the eta-expansion has to happen at the
    /// comparison: apply the constructor to its own parameters and ask about
    /// the result. Without this no type lambda could satisfy any bound, and
    /// cats' `instance[F[_, _] <: Product]` at `(A0, *, *)` was rejected with
    /// "inferred type arguments … do not conform".
    ///
    /// Only reached when the plain comparison already said no, so it can widen
    /// what is accepted but never narrow it. A class used as a constructor
    /// (`Box[Tuple2]`) still goes through the ordinary path.
    pub(crate) fn hk_ctor_meets_proper_bound(&self, ctor: &Type, bound: &Type) -> bool {
        let n = self.kind_arity(ctor);
        if n == 0 || self.kind_arity(bound) != 0 {
            return false;
        }
        let Some((params, _)) = self.hk_alias(ctor) else {
            return false;
        };
        if params.len() != n {
            return false;
        }
        let args: Vec<Type> = params.iter().map(|p| Type::TypeParam(*p)).collect();
        let applied = self.expand_applied_hk_alias(apply_type_ctor(ctor.clone(), args));
        self.is_sub_type(&applied, bound)
    }

    /// `Class { sym: array_sym, args: [T] }` re-spelled as `Type::Array(T)`.
    ///
    /// `None` for anything else, including the bare `Array` constructor, which
    /// has no element yet.
    pub fn array_class_form(&self, ty: &Type) -> Option<Type> {
        match ty {
            Type::Class { sym, args } if *sym == self.array_sym && args.len() == 1 => {
                Some(Type::Array(Box::new(args[0].clone())))
            }
            _ => None,
        }
    }

    /// True for `scala.Array` as a *class symbol*.
    ///
    /// `array_sym` alone is not enough. When the standard library's own
    /// `src/library/scala/Array.scala` is one of the sources under
    /// compilation, `shadow_supplied_by_source` takes the prelude's `Array`
    /// out of every scope and puts the source class there instead, but
    /// deliberately leaves `array_sym` pointing at the prelude symbol —
    /// its id is written into prelude signatures that are still in use.
    /// So `new Array(n)` in that run names a symbol that *is* `scala.Array`
    /// and is not `array_sym`, and asking only about `array_sym` answered
    /// "no" for every one of the library's own 64 `new Array(n)`s.
    pub fn is_array_class(&self, sym: SymbolId) -> bool {
        if sym.is_none() {
            return false;
        }
        sym == self.array_sym
            || (self.get(sym).name == "Array"
                && self.get(sym).owner == self.scala_pkg
                && self.get(sym).kind == SymKind::Class)
    }

    /// How many type parameters a class declares.
    ///
    /// `scala.Array` is the one class whose parameter is not in the symbol
    /// table: source `Array[T]` becomes `Type::Array`, so the symbol carries no
    /// `T`. Its kind is still `* -> *` -- `TypedCollectionTypeConstructor[Array]`
    /// (slick's `ast/Type.scala`) passes it as a type *constructor* -- so the
    /// count has to say 1 or every such use is rejected as a kind error.
    pub fn class_tparam_count(&self, sym: SymbolId) -> usize {
        if sym == self.array_sym {
            1
        } else {
            self.get(sym).tparams.len()
        }
    }

    /// Remaining kind arity: 0 is a proper type (`*`), 1 is `* -> *`, etc.
    pub fn kind_arity(&self, ty: &Type) -> usize {
        match ty {
            // An inner class behind a prefix (`prefix.rs`) has the kind of
            // the class: `Accumulator[List]` for `class List[T]` inside `HOSeq`.
            Type::Refined { .. } if crate::prefix::view_prefix(ty).is_some() => {
                self.kind_arity(crate::prefix::strip_view(ty))
            }
            Type::TypeParam(id) | Type::TypeMember(id) => self.get(*id).tparams.len(),
            Type::Class { sym, args } => self.class_tparam_count(*sym).saturating_sub(args.len()),
            Type::Applied { ctor, args } => self.kind_arity(ctor).saturating_sub(args.len()),
            Type::Named { args, .. } => {
                if args.is_empty() {
                    self.class_sym_of(ty)
                        .map(|c| self.class_tparam_count(c))
                        .unwrap_or(0)
                } else {
                    0
                }
            }
            Type::Annotated { tpe, .. } => self.kind_arity(tpe),
            _ => 0,
        }
    }

    /// Kinds of the next type parameters of a type constructor (`F[_]` → `[0]`).
    pub fn tparam_arities(&self, ty: &Type) -> Vec<usize> {
        match ty {
            Type::Refined { .. } if crate::prefix::view_prefix(ty).is_some() => {
                self.tparam_arities(crate::prefix::strip_view(ty))
            }
            // `Array`'s element parameter is a proper type and is not in the
            // symbol table (see `class_tparam_count`).
            Type::Class { sym, args } if *sym == self.array_sym => {
                vec![0; 1usize.saturating_sub(args.len())]
            }
            Type::Class { sym, args } => self
                .get(*sym)
                .tparams
                .iter()
                .skip(args.len())
                .map(|tp| self.get(*tp).tparams.len())
                .collect(),
            Type::TypeParam(id) | Type::TypeMember(id) => self
                .get(*id)
                .tparams
                .iter()
                .map(|tp| self.get(*tp).tparams.len())
                .collect(),
            Type::Applied { ctor, args } => {
                let mut rest = self.tparam_arities(ctor);
                let n = args.len().min(rest.len());
                rest.drain(0..n);
                rest
            }
            Type::Annotated { tpe, .. } => self.tparam_arities(tpe),
            _ => Vec::new(),
        }
    }

    /// Classes `ty` names through `this.type`, in no particular order.
    pub(crate) fn this_type_owners(ty: &Type, out: &mut Vec<SymbolId>) {
        match ty {
            Type::ThisType(c) => {
                if !out.contains(c) {
                    out.push(*c);
                }
            }
            Type::Class { args, .. } | Type::Tuple(args) => {
                for a in args {
                    Self::this_type_owners(a, out);
                }
            }
            Type::Applied { ctor, args } => {
                Self::this_type_owners(ctor, out);
                for a in args {
                    Self::this_type_owners(a, out);
                }
            }
            Type::Array(t) | Type::ByName(t) | Type::Repeated(t) => Self::this_type_owners(t, out),
            Type::Annotated { tpe, .. } => Self::this_type_owners(tpe, out),
            Type::Function { params, ret } => {
                for p in params {
                    Self::this_type_owners(p, out);
                }
                Self::this_type_owners(ret, out);
            }
            Type::Method { paramss, ret } => {
                for ps in paramss {
                    for p in ps {
                        Self::this_type_owners(p, out);
                    }
                }
                Self::this_type_owners(ret, out);
            }
            // A view's prefix (`prefix.rs`) and a path's own prefix are
            // where `C.this` is written for an inner class or a member path:
            // `def send(m: in.Message)` read through `unstable` is
            // `unstable.in.Message`.
            Type::SingleType { prefix, .. } => Self::this_type_owners(prefix, out),
            Type::Refined { parents, decls } => {
                for p in parents {
                    Self::this_type_owners(p, out);
                }
                for d in decls {
                    if let RefineDecl::Type { rhs: Some(t), .. } = d {
                        Self::this_type_owners(t, out);
                    }
                }
            }
            _ => {}
        }
    }

    /// The class whose `this` an unqualified reference to a member of `owner`
    /// is selected on: the innermost class enclosing the current one that is
    /// `owner` or has it as a base class -- through its self type too
    /// (`is_ancestor_of`), which is how a component trait with
    /// `self: Profile =>` reaches `Profile`'s members. `None` when no
    /// enclosing class does (a member reached some other way, an import).
    pub(crate) fn enclosing_class_reaching(&self, owner: SymbolId) -> Option<SymbolId> {
        let mut c = self.this_class;
        while !c.is_none() {
            if matches!(
                self.get(c).kind,
                SymKind::Class | SymKind::ModuleClass | SymKind::Module
            ) && (c == owner || self.is_ancestor_of(owner, c))
            {
                return Some(c);
            }
            c = self.get(c).owner;
        }
        None
    }

    /// Parent/name lookups may see truncated bounds or aliases under an
    /// ambient expansion guard. Such answers must not share a memo with a
    /// lookup made outside that expansion, even while `self` is immutable.
    pub(crate) fn has_ambient_type_context(&self) -> bool {
        EXPANDING_BOUNDS.with(|s| !s.borrow().is_empty())
            || EXPANDING_ALIASES.with(|s| !s.borrow().is_empty())
            || CHASING.with(|s| !s.borrow().is_empty())
            || WRITTEN_TYPE.with(|s| s.get())
            || SUBTYPE_WALK.with(|s| s.borrow().depth != 0)
            || !self.qualify_tparams.borrow().is_empty()
    }

    pub(crate) fn is_ancestor_of(&self, anc: SymbolId, cls: SymbolId) -> bool {
        let mut work = vec![cls];
        let mut seen = rustc_hash::FxHashSet::default();
        while let Some(c) = work.pop() {
            if !seen.insert(c.0) {
                continue;
            }
            if c == anc {
                return true;
            }
            for p in &self.get(c).parents {
                if let Some(ps) = self.class_sym_of(p) {
                    work.push(ps);
                }
            }
            if let Some(st) = &self.get(c).self_type {
                work.extend(self.self_type_classes(st));
            }
        }
        false
    }

    /// Substitute inherited member types using applied parents (`Functor[Id].map`).
    pub fn subst_as_seen_from(&self, recv: &Type, ty: &Type) -> Type {
        // `this.type` in a member's signature means the receiver it was
        // selected on: `def add(v: T): this.type` on a `B[String]` gives back a
        // `B[String]`, not a bare `B` whose argument has to be invented.
        self.subst_as_seen_from_at(recv, None, ty)
    }

    /// `subst_as_seen_from` for a receiver whose *own* type is not the class
    /// type it is read through: `x.m` on `x: C` with `C <: Ops[A, C]`. Members
    /// are still read at `Ops[A, C]`'s arguments, but a `this.type` result is
    /// the prefix's type (SLS 3.2.1: seen from an unstable prefix of type `C`,
    /// `this.type` is `C`), so `clone() -= key` in `MapOps` is a `C` and not a
    /// `MapOps[K, V, CC, C]`.
    ///
    /// The `this.type` is replaced *after* the walk: `prefix` is written in
    /// the caller's vocabulary, and the walk substitutes the receiver class's
    /// type parameters, which may share symbols with it.
    pub fn subst_as_seen_from_prefix(&self, recv: &Type, prefix: &Type, ty: &Type) -> Type {
        let mut owners = Vec::new();
        Self::this_type_owners(ty, &mut owners);
        let Some(rc) = self.class_sym_of(recv) else {
            return self.subst_as_seen_from(recv, ty);
        };
        owners.retain(|&c| c == rc || self.is_ancestor_of(c, rc));
        if owners.is_empty() {
            return self.subst_as_seen_from(recv, ty);
        }
        let inner_pre = matches!(prefix, Type::ThisType(_)).then_some(prefix);
        let ty = self.rewrite_view_this(recv, inner_pre, ty);
        let mut out = self.subst_as_seen_from_walk_at(recv, inner_pre, &ty);
        for c in owners {
            out = subst_this_type(&out, c, prefix);
        }
        out
    }

    /// Replaces `this.type` of the receiver's class (and its ancestors) in
    /// `ty` by `to`.
    fn subst_receiver_this_type(&self, recv: &Type, to: &Type, ty: &Type) -> Type {
        let mut owners = Vec::new();
        Self::this_type_owners(ty, &mut owners);
        let mut out = ty.clone();
        if !owners.is_empty() && !matches!(recv, Type::ThisType(_)) {
            if let Some(rc) = self.class_sym_of(recv) {
                for c in owners {
                    if c == rc || self.is_ancestor_of(c, rc) {
                        out = subst_this_type(&out, c, to);
                    }
                }
            }
        }
        out
    }

    /// `subst_as_seen_from` with the prefix bare inner classes in `ty` are
    /// given: the stable path the member was selected on, when the caller
    /// has one (`o.mk` for `def mk: In` is an `o.In`, not an `Outer#In`).
    pub fn subst_as_seen_from_at(&self, recv: &Type, inner_pre: Option<&Type>, ty: &Type) -> Type {
        let ty = self.rewrite_view_this(recv, inner_pre, ty);
        let ty = self.subst_receiver_this_type(recv, recv, &ty);
        self.subst_as_seen_from_walk_at(recv, inner_pre, &ty)
    }

    /// The parent walk of `subst_as_seen_from`, without its `this.type` step.
    fn subst_as_seen_from_walk_at(&self, recv: &Type, inner_pre: Option<&Type>, ty: &Type) -> Type {
        fn walk(
            st: &SymbolTable,
            recv: &Type,
            ty: Type,
            seen: &mut rustc_hash::FxHashSet<u32>,
            base: &BaseTypeArgs,
        ) -> Type {
            match recv {
                Type::Class { sym, args } => {
                    if !seen.insert(sym.0) {
                        return ty;
                    }
                    // A base class reachable through two parents has to be read
                    // at its **most derived** instantiation, and this walk takes
                    // whichever path reaches it first. `base` decides that
                    // independently of the path (see [`base_type_args`]), so the
                    // arguments the walk arrived with are only used for a class
                    // the receiver does not have as a base -- one reached
                    // through a self type, a bound, or a refinement.
                    let args = base.get(&sym.0).unwrap_or(args);
                    let mut t = if args.is_empty() {
                        ty
                    } else {
                        st.subst_tparams(*sym, args, &ty)
                    };
                    for i in 0..st.get(*sym).parents.len() {
                        // The parent is declared in terms of *this* class's
                        // type parameters, so it has to be instantiated before
                        // it can instantiate anything itself. Without this,
                        // `OptionMapper2[B1, B2, Boolean, P1, P2, R].column`
                        // keeps its `implicit TypedType[BR]` raw instead of
                        // resolving `BR` to `Boolean` through
                        // `OptionMapper[BR, R]`.
                        let p = st.subst_tparams_cow(*sym, args, &st.get(*sym).parents[i]);
                        t = walk(st, &p, t, seen, base);
                    }
                    // A self type is a second place `this` inherits members
                    // from, and they are declared in *its* vocabulary:
                    // `trait P[A] { self: Q[A] => def p: A = q }` reads `q`
                    // out of `Q`, whose own `A` has to become `P`'s. Without
                    // this the two `A`s printed the same and compared
                    // unequal -- "type mismatch; found: A required: A".
                    if let Some(sf) = &st.get(*sym).self_type {
                        let sf = st.subst_tparams_cow(*sym, args, sf);
                        t = walk(st, &sf, t, seen, base);
                    }
                    t
                }
                Type::ModuleRef(sym) => {
                    if !seen.insert(sym.0) {
                        return ty;
                    }
                    let mut t = ty;
                    for p in st.get(*sym).parents.clone() {
                        t = walk(st, &p, t, seen, base);
                    }
                    t
                }
                Type::Annotated { tpe, .. } => walk(st, tpe, ty, seen, base),
                // `p.type` has the members of `p`'s type, at its arguments:
                // `def f(b: Buf[Int]): b.type` and then `f(x).add(1)` reads
                // `add` through `Buf[Int]`. Without this the walk stopped here
                // and `add` kept its declared `(A)`. (`this.type` in the member
                // has already become `p.type` above, which is the point of
                // keeping the singleton as the receiver.)
                // The prefix is where the term is *declared*, not what it is,
                // so a term with no type yet gives nothing to walk.
                Type::SingleType { sym, .. } => {
                    if !seen.insert(sym.0) {
                        return ty;
                    }
                    let under = st.singleton_underlying(*sym);
                    if under.is_no_type() || matches!(under, Type::Method { .. }) {
                        ty
                    } else {
                        walk(st, &under, ty, seen, base)
                    }
                }
                // `trait C[-T] extends (T => R)` inherits `Function1.apply`,
                // and reading its type through `C[X]` means walking into
                // `Function1[X, R]`. A structural function names no class, so
                // it has to be read back as one first.
                Type::Function { .. } => match st.function_class_form(recv) {
                    Some(c) => walk(st, &c, ty, seen, base),
                    None => ty,
                },
                // `trait GetResult[+T] extends (PositionedResult => T) { self => }`
                // and then `self.apply(rs)`: the receiver is the class's own
                // `this`, so the member has to be read through the class's
                // parents at the class's own type parameters. Without this the
                // inherited `Function1.apply` kept `T1` and `R`.
                Type::ThisType(sym) => {
                    let args: Vec<Type> = st
                        .get(*sym)
                        .tparams
                        .iter()
                        .map(|t| Type::TypeParam(*t))
                        .collect();
                    walk(st, &Type::Class { sym: *sym, args }, ty, seen, base)
                }
                // Only heads that `apply_type_ctor` folds may be re-walked: an
                // abstract type-member head (`ColumnType[U]`) folds to the very
                // same `Applied`, so recursing on it would not terminate — and
                // it carries no class parameters to substitute anyway.
                Type::Applied { ctor, args }
                    if matches!(
                        ctor.as_ref(),
                        Type::Class { .. } | Type::Named { .. } | Type::Applied { .. }
                    ) =>
                {
                    let t = apply_type_ctor((**ctor).clone(), args.clone());
                    if matches!(t, Type::Applied { .. }) {
                        // Still applied: the constructor is abstract (a type
                        // member or parameter), so it names no class to walk
                        // into. Recursing here would not terminate.
                        return ty;
                    }
                    walk(st, &t, ty, seen, base)
                }
                // A member reached through `Ops[F, A] { type TypeClassType =
                // FlatMap[F] }` is declared by one of the parents and has to be
                // read at that parent's arguments. Without this, cats' whole
                // syntax layer -- every result type simulacrum writes is a
                // refinement -- handed back `flatMap`'s raw `A`.
                Type::Refined { parents, .. } => {
                    let mut t = ty;
                    for p in parents {
                        t = walk(st, p, t, seen, base);
                    }
                    // An inner class seen through a prefix, reached as the
                    // type of a path (`m.In` with `val m = o.mid`): the
                    // prefix instantiates the enclosing class. Its own
                    // `seen`, so the enclosing class is walked even when the
                    // inner class also derives from it, and so the classes
                    // it reaches are not the receiver's own (see
                    // `attach_inner_prefixes`).
                    if let Some(pre) = crate::prefix::view_prefix(recv) {
                        let mut pseen = rustc_hash::FxHashSet::default();
                        t = walk(st, pre, t, &mut pseen, base);
                    }
                    t
                }
                // A member reached through an abstract type member (or a type
                // parameter) is declared by its *upper bound*, and the bound is
                // where the arguments to substitute are written. The reflect
                // API is nothing but these: `type MemberScope >: Null <: Scope
                // with MemberScopeApi`, `type Scope >: Null <: ScopeApi`, and
                // `ScopeApi extends Iterable[Symbol]`. Stopping here handed
                // `decls.toList` back `Iterable`'s own `List[A]` -- with `A`
                // unbound, so the element type slick's `mapToImpl` enumerates
                // case-class fields with was never `Symbol`.
                Type::TypeMember(id) | Type::TypeParam(id) => {
                    if !seen.insert(id.0) {
                        return ty;
                    }
                    match st.get(*id).bound_hi.clone() {
                        Some(hi) => walk(st, &hi, ty, seen, base),
                        None => ty,
                    }
                }
                _ => ty,
            }
        }
        // A receiver that is an inner class seen through a prefix (`o.In`,
        // `prefix.rs`) is walked as the class, and then the prefix is walked
        // in turn: the members of `In` are written in `Outer`'s vocabulary
        // as much as in `In`'s own, and the prefix is what instantiates
        // `Outer`. Its own `seen` set, so a `T` of `Outer` that `In`'s own
        // base walk did not reach is still substituted.
        let outer = crate::prefix::view_prefix(recv).or_else(|| {
            // An object nested in a class, selected through a path (`o.Rec`
            // for `case class Rec` inside `Outer`, whose companion's `apply`
            // takes `Outer`'s `T`): the path's prefix is its enclosing
            // instance.
            match inner_pre {
                Some(Type::SingleType { prefix, sym })
                    if self.is_singleton_prefix(prefix)
                        && Some(*sym) == self.class_sym_of(recv)
                        && self.get(*sym).kind == SymKind::ModuleClass
                        && self.get(self.get(*sym).owner).kind == SymKind::Class =>
                {
                    Some(prefix.as_ref())
                }
                _ => None,
            }
        });
        let core = if crate::prefix::view_prefix(recv).is_some() {
            crate::prefix::strip_view(recv)
        } else {
            recv
        };
        // Only a receiver that names a class outright is given a base map:
        // `class_sym_of` chases bounds and aliases, and the walk's own
        // recursion re-enters here for those shapes anyway.
        let base = match core {
            Type::Class { sym, args } if !sym.is_none() => self.base_type_args(*sym, args),
            Type::ModuleRef(sym) | Type::ThisType(sym) if !sym.is_none() => {
                self.base_type_args(*sym, &[])
            }
            Type::SingleType { sym, .. } => match self.singleton_underlying(*sym) {
                Type::Class { sym, args } if !sym.is_none() => self.base_type_args(sym, &args),
                _ => self.empty_base_type_args.clone(),
            },
            // A compound (`C <: LinearSeq[A] with LinearSeqOps[A, CC, C]`) is
            // a base-type sequence of its own, merged across the parents
            // rather than taken from whichever one the walk reaches first.
            Type::Refined { parents, .. } if parents.len() > 1 => {
                std::rc::Rc::new(self.base_type_args_compound(parents))
            }
            _ => self.empty_base_type_args.clone(),
        };
        let mut seen = rustc_hash::FxHashSet::default();
        let t = walk(self, core, ty.clone(), &mut seen, &base);
        // nsc's as-seen-from for the enclosing instance: a bare inner class
        // of any class the walk went through means `C.this.In`, and read
        // through this receiver it is the receiver's `In`. The receiver is
        // the prefix unless the caller named a better one (`inner_pre`: the
        // stable path the member was selected on).
        let pre = match inner_pre {
            Some(p) => p.clone(),
            None => self.canonical_prefix(recv),
        };
        let t = self.attach_inner_prefixes(&seen, &pre, t);
        match outer {
            Some(o) => self.subst_as_seen_from_walk_at(o, None, &t),
            None => t,
        }
    }

    /// Every base class of `sym[args]`, mapped to the type arguments it is
    /// instantiated at *there* — the answer nsc reads out of a `BaseTypeSeq`.
    ///
    /// A base class reached through two parents at two different
    /// instantiations must be read at the **most derived** of them, and SLS
    /// 5.1.2's linearization is what decides which that is. Walking parents in
    /// written order does not: `final class HashMap[K, +V] extends
    /// AbstractMap[K, V] with StrictOptimizedMapOps[K, V, HashMap,
    /// HashMap[K, V]]` reaches `MapOps` as `MapOps[K, V, Map, Map[K, V]]`
    /// through `AbstractMap` and as `MapOps[K, V, HashMap, HashMap[K, V]]`
    /// through the mixin, and the written order takes the first — so
    /// `updatedWith` came back as `Map[K, V1]` where `HashMap[K, V1]` is what
    /// scalac 2.13.16 types it as.
    ///
    /// Keeping only the instantiation the most derived *reacher* supplies is
    /// not the same thing, and the difference is what this walk used to get
    /// wrong. Two parents can reach one base without either standing above the
    /// other: `trait Str[+A] extends LinOps[A, Str[A]] with Iter[A]` reaches
    /// `IterOps` as `IterOps[A, Str[A]]` through `LinOps` and as `IterOps[A,
    /// Iter[A]]` through `Iter`, and SLS 5.1.2 puts `Iter` first because it is
    /// written last. Taking the first arrival read `IterOps.tail` as
    /// `Iter[A]`, where scalac 2.13.16 types it `Str[A]` — the eight-line
    /// `Str` above, and `Stream` in the standard library, which reaches
    /// `IterableOps` as both `IterableOps[A, Stream, Stream[A]]` and
    /// `IterableOps[A, Iterable, Iterable[A]]`.
    ///
    /// nsc's answer is [`meet_base_args`]: `BaseTypeSeqs.compoundBaseTypeSeq`
    /// keeps *every* variant a base class is reached at and resolves the entry
    /// with `mergePrefixAndArgs(variants, Variance.Contravariant, _)`, which
    /// takes the glb at a covariant parameter and the lub at a contravariant
    /// one. The order the variants are collected in does not matter to that,
    /// which is why nothing here reorders the linearization —
    /// `agent/basetypeseq` measured that (scala library 1551 → 1579) and threw
    /// it away.
    ///
    /// `linearize` lists every class before all of its own ancestors, so by
    /// the time the walk reaches a class every parent clause that names it has
    /// already been substituted and recorded, and its entry can be settled on
    /// the spot. Every entry is expressed in `args`' vocabulary, so a caller
    /// substitutes with it directly and needs no second pass.
    pub(crate) fn base_type_args(&self, sym: SymbolId, args: &[Type]) -> std::rc::Rc<BaseTypeArgs> {
        if let Some(hit) = self.cached_base_type_args(sym, args) {
            return hit;
        }
        // The walk reads the parent clauses of every class in `sym`'s
        // linearization, so it is only a function of the symbol graph where
        // that linearization is (`crate::lin::linearize_settled`): a class with
        // an unresolved parent is read differently once the name binds, and
        // nothing mutates a symbol when it does.
        let (lin, settled) = crate::lin::linearize_settled(self, sym);
        let out = std::rc::Rc::new(self.base_type_args_uncached(sym, args, &lin));
        if settled {
            self.cache_base_type_args(sym, args, &out);
        }
        out
    }

    fn base_type_args_uncached(
        &self,
        sym: SymbolId,
        args: &[Type],
        lin: &[SymbolId],
    ) -> BaseTypeArgs {
        // One entry per base class, holding the instantiation settled on and,
        // only for a class two parent clauses disagree about, the further
        // instantiations still to merge (nsc's `minTypes`). One map and one
        // hash lookup per parent clause: this walk runs on every
        // `subst_as_seen_from`, and a second map cost 20% of the compiler.
        let mut slots: rustc_hash::FxHashMap<u32, BaseSlot> = rustc_hash::FxHashMap::default();
        let mut unmerged = 0usize;
        // Ancestry answers this walk has already paid for. The same pair --
        // `Stream` against `Iterable`, say -- decides several argument
        // positions of several base classes, and each answer is a walk of the
        // parent DAG.
        let mut anc: AncMemo = Vec::new();
        slots.insert(
            sym.0,
            BaseSlot {
                args: args.to_vec(),
                settled: true,
                more: Vec::new(),
            },
        );
        for &c in lin {
            // Lifted out rather than cloned: the arguments go back into the
            // slot after the parent clauses have been substituted with them,
            // and a `Vec<Type>` clone per class of every linearization was one
            // of the typer's larger sources of allocation.
            let cargs = {
                let Some(slot) = slots.get_mut(&c.0) else {
                    continue;
                };
                if !slot.settled {
                    slot.settled = true;
                    if !slot.more.is_empty() {
                        unmerged -= 1;
                        let mut vs = Vec::with_capacity(slot.more.len() + 1);
                        vs.push(std::mem::take(&mut slot.args));
                        vs.append(&mut slot.more);
                        slot.args = self.meet_base_args(c, &vs, &mut anc);
                    }
                }
                std::mem::take(&mut slot.args)
            };
            for p in &self.get(c).parents {
                // A parent written as a function type is `scala.FunctionN`,
                // which is how `linearize` reads it too.
                let as_class = self.function_class_form(p);
                let p = self.subst_tparams_cow(c, &cargs, as_class.as_ref().unwrap_or(p));
                if let Type::Class {
                    sym: ps,
                    args: pargs,
                } = &*p
                {
                    match slots.entry(ps.0) {
                        std::collections::hash_map::Entry::Vacant(v) => {
                            v.insert(BaseSlot {
                                args: pargs.clone(),
                                settled: false,
                                more: Vec::new(),
                            });
                        }
                        std::collections::hash_map::Entry::Occupied(mut o) => {
                            let slot = o.get_mut();
                            // Already settled: the linearization lists a class
                            // before all of its ancestors, so a clause
                            // arriving after the entry was taken is a cycle,
                            // not a variant. A base class with no type
                            // arguments has nothing to merge either.
                            if slot.settled || pargs.is_empty() {
                                continue;
                            }
                            if &slot.args != pargs && !slot.more.iter().any(|v| v == pargs) {
                                if slot.more.is_empty() {
                                    unmerged += 1;
                                }
                                slot.more.push(pargs.clone());
                            }
                        }
                    }
                }
            }
            if let Some(slot) = slots.get_mut(&c.0) {
                slot.args = cargs;
            }
        }
        // `linearize` drops `Any`/`AnyRef`/`AnyVal`/`Object`, so those never
        // come round as walk heads; they still get the entry the parent
        // clauses named them at, as they did when this walk inserted every
        // parent directly.
        if unmerged > 0 {
            let pending: Vec<u32> = slots
                .iter()
                .filter(|(_, s)| !s.settled && !s.more.is_empty())
                .map(|(k, _)| *k)
                .collect();
            for k in pending {
                let slot = slots.get_mut(&k).expect("just listed");
                slot.settled = true;
                let mut vs = Vec::with_capacity(slot.more.len() + 1);
                vs.push(std::mem::take(&mut slot.args));
                vs.append(&mut slot.more);
                slot.args = self.meet_base_args(SymbolId(k), &vs, &mut anc);
            }
        }
        BaseTypeArgs { slots }
    }

    /// [`Self::base_type_args`] for a **compound** type's parent list.
    ///
    /// nsc builds a `RefinedType`'s base-type sequence with
    /// `GlbBaseTypeSeq`, which merges the parents' sequences exactly as
    /// `compoundBaseTypeSeq` merges one class's parent clauses. Without it a
    /// compound *bound* was read through whichever parent came first:
    ///
    /// ```scala
    /// trait LinearSeqOps[+A, +CC[X] <: LinearSeq[X],
    ///                    +C <: LinearSeq[A] with LinearSeqOps[A, CC, C]]
    ///   extends SeqOps[A, CC, C] { def tail: C; … }
    /// var these = coll      // C
    /// these = these.tail    // C, through `LinearSeqOps[A, CC, C]`
    /// ```
    ///
    /// `LinearSeq[A]` is written first and reaches `LinearSeqOps` as
    /// `LinearSeqOps[A, LinearSeq, LinearSeq[A]]`, so `tail` came back a
    /// `LinearSeq[A]` and the assignment was "type mismatch; found:
    /// LinearSeq[A] required: C" (`scala/collection/LinearSeq.scala`,
    /// `Seq.scala`). The merge takes the glb at a covariant parameter, and
    /// `C <: LinearSeq[A]`, so `C` is what the member is read at — the same
    /// answer nsc gives, reached the same way and independent of the order
    /// the parents are written in.
    pub(crate) fn base_type_args_compound(&self, parents: &[Type]) -> BaseTypeArgs {
        let mut variants: rustc_hash::FxHashMap<u32, Vec<Vec<Type>>> =
            rustc_hash::FxHashMap::default();
        for p in parents {
            let as_class = self.function_class_form(p);
            let Type::Class { sym, args } = as_class.as_ref().unwrap_or(p) else {
                continue;
            };
            if sym.is_none() {
                continue;
            }
            for (k, slot) in self.base_type_args(*sym, args).slots.iter() {
                let vs = variants.entry(*k).or_default();
                if !vs.contains(&slot.args) {
                    vs.push(slot.args.clone());
                }
            }
        }
        let mut anc: AncMemo = Vec::new();
        let slots = variants
            .into_iter()
            .map(|(k, vs)| {
                let args = self.meet_base_args(SymbolId(k), &vs, &mut anc);
                (
                    k,
                    BaseSlot {
                        args,
                        settled: true,
                        more: Vec::new(),
                    },
                )
            })
            .collect();
        BaseTypeArgs { slots }
    }

    /// nsc's `mergePrefixAndArgs(variants, Variance.Contravariant, _)`, read
    /// off the argument lists: the arguments a base class reached at several
    /// instantiations is seen at.
    ///
    /// Position by position, and by the *parameter's* variance against the
    /// contravariant direction the merge is asked in: a covariant parameter
    /// takes the glb (the most derived of the arrivals), a contravariant one
    /// the lub (the least derived). An invariant parameter reached at two
    /// different arguments is an illegal inheritance, which nsc answers with
    /// an existential over `TypeBounds(glb, lub)`; the first arrival is kept
    /// instead, so a program that does not have that error is unaffected and
    /// one that does is left to the inheritance check to report.
    fn meet_base_args(
        &self,
        cls: SymbolId,
        variants: &[Vec<Type>],
        anc: &mut AncMemo,
    ) -> Vec<Type> {
        let Some(first) = variants.first() else {
            return Vec::new();
        };
        let n = first.len();
        if variants.len() < 2
            || self.get(cls).tparams.len() != n
            || variants.iter().any(|v| v.len() != n)
        {
            return first.clone();
        }
        let mut out = Vec::with_capacity(n);
        let mut column: Vec<&Type> = Vec::with_capacity(variants.len());
        for i in 0..n {
            column.clear();
            column.extend(variants.iter().map(|v| &v[i]));
            let tp = self.get(cls).tparams[i];
            out.push(match self.tparam_variance(tp) {
                Some(true) => self.meet_type(&column, true, 0, anc),
                Some(false) => self.meet_type(&column, false, 0, anc),
                None => first[i].clone(),
            });
        }
        out
    }

    /// `Some(true)` covariant, `Some(false)` contravariant, `None` invariant.
    fn tparam_variance(&self, tp: SymbolId) -> Option<bool> {
        let f = self.get(tp).flags;
        if f.contains(Flags::COVARIANT) {
            Some(true)
        } else if f.contains(Flags::CONTRAVARIANT) {
            Some(false)
        } else {
            None
        }
    }

    /// The glb (`most_derived`) or lub of one argument position's arrivals.
    ///
    /// Only the two shapes a base-type merge actually produces are computed:
    /// the same class at different arguments, which recurses position by
    /// position and flips at a contravariant parameter exactly as nsc's
    /// `glb`/`lub` do; and different classes on one inheritance chain, where
    /// the answer is the arrival every other arrival is an ancestor of. When
    /// there is no such arrival the merge has no answer inside a class
    /// hierarchy either — nsc would build an intersection — and the first is
    /// kept, which is what this walk always did.
    fn meet_type(
        &self,
        cands: &[&Type],
        most_derived: bool,
        depth: u32,
        anc: &mut AncMemo,
    ) -> Type {
        let Some(first) = cands.first().copied() else {
            return Type::NoType;
        };
        if depth > 4 || cands.len() < 2 {
            return first.clone();
        }
        // Almost every position of a merged base type is the same argument in
        // every arrival -- only the `C` and `CC` of a collection actually
        // differ -- so answering that case without allocating is what keeps
        // this off the profile.
        if cands[1..].iter().all(|c| Self::eq_unannotated(first, c)) {
            let plain = cands.iter().find(|c| !Self::has_annotation(c));
            return plain.copied().unwrap_or(first).clone();
        }
        // Two arrivals that differ only by a type annotation are one type --
        // nsc's `=:=` does not look at `@uncheckedVariance` -- and the
        // unannotated spelling is the one to keep. `IterableFactoryDefaults[+A,
        // +CC[x]] extends IterableOps[A, CC, CC[A @uncheckedVariance]]` is
        // reached before `SeqOps`' `IterableOps[A, CC, C]` in `Stream`'s
        // linearization, and keeping the annotation would make
        // `IterableOps.tail` read `Stream[A @uncheckedVariance]` where scalac
        // 2.13.16 prints `Stream[A]`. Only a merge does this: a base class
        // reached once keeps the spelling its one parent clause wrote.
        let mut uniq: Vec<&Type> = Vec::with_capacity(cands.len());
        for c in cands {
            match uniq.iter().position(|u| Self::eq_unannotated(u, c)) {
                Some(k) => {
                    if !Self::has_annotation(c) {
                        uniq[k] = c;
                    }
                }
                None => uniq.push(c),
            }
        }
        if uniq.len() < 2 {
            return uniq.first().copied().unwrap_or(first).clone();
        }
        // One representative per class: the same class reached at different
        // arguments merges position by position, flipping at a contravariant
        // parameter exactly as nsc's `glb`/`lub` do. Only then can the
        // representatives be ordered against each other by ancestry.
        let mut reps: Vec<Type> = Vec::with_capacity(uniq.len());
        for t in &uniq {
            let head = self.base_arg_class(t);
            let at = head.and_then(|h| reps.iter().position(|r| self.base_arg_class(r) == Some(h)));
            match at {
                Some(k) => reps[k] = self.merge_same_head(&reps[k], t, most_derived, depth, anc),
                None => reps.push((*t).clone()),
            }
        }
        if reps.len() == 1 {
            return reps.remove(0);
        }
        for (i, c) in reps.iter().enumerate() {
            let wins = reps
                .iter()
                .enumerate()
                .all(|(j, o)| i == j || self.arg_outranks(c, o, most_derived, anc));
            if wins {
                return c.clone();
            }
        }
        first.clone()
    }

    /// `a` and `b` name the same class: merge their arguments.
    fn merge_same_head(
        &self,
        a: &Type,
        b: &Type,
        most_derived: bool,
        depth: u32,
        anc: &mut AncMemo,
    ) -> Type {
        let (Type::Class { sym, args: a1 }, Type::Class { args: a2, .. }) = (a, b) else {
            return a.clone();
        };
        if a1.is_empty() || a1.len() != a2.len() || self.get(*sym).tparams.len() != a1.len() {
            return a.clone();
        }
        let args = (0..a1.len())
            .map(|i| {
                let col = [&a1[i], &a2[i]];
                match self.tparam_variance(self.get(*sym).tparams[i]) {
                    Some(true) => self.meet_type(&col, most_derived, depth + 1, anc),
                    Some(false) => self.meet_type(&col, !most_derived, depth + 1, anc),
                    None => a1[i].clone(),
                }
            })
            .collect();
        Type::Class { sym: *sym, args }
    }

    /// Whether `a` and `b` are the same type written with different
    /// annotations. Structural and allocation-free: this runs inside the
    /// base-type walk, which runs on every `subst_as_seen_from`.
    fn eq_unannotated(a: &Type, b: &Type) -> bool {
        match (a, b) {
            (Type::Annotated { tpe, .. }, _) => Self::eq_unannotated(tpe, b),
            (_, Type::Annotated { tpe, .. }) => Self::eq_unannotated(a, tpe),
            (Type::Class { sym: s1, args: a1 }, Type::Class { sym: s2, args: a2 }) => {
                s1 == s2
                    && a1.len() == a2.len()
                    && a1.iter().zip(a2).all(|(x, y)| Self::eq_unannotated(x, y))
            }
            (Type::Applied { ctor: c1, args: a1 }, Type::Applied { ctor: c2, args: a2 }) => {
                Self::eq_unannotated(c1, c2)
                    && a1.len() == a2.len()
                    && a1.iter().zip(a2).all(|(x, y)| Self::eq_unannotated(x, y))
            }
            _ => a == b,
        }
    }

    /// Whether `ty` carries a type annotation anywhere, so the plainer of two
    /// spellings of one type can be preferred.
    fn has_annotation(ty: &Type) -> bool {
        match ty {
            Type::Annotated { .. } => true,
            Type::Class { args, .. } => args.iter().any(Self::has_annotation),
            Type::Applied { ctor, args } => {
                Self::has_annotation(ctor) || args.iter().any(Self::has_annotation)
            }
            _ => false,
        }
    }

    /// The declared upper bound of an abstract type (a type parameter or a
    /// deferred type member), and `None` for everything else -- including an
    /// unbounded parameter, which nothing can order.
    fn abstract_bound(&self, t: &Type) -> Option<Type> {
        match t {
            Type::TypeParam(id) | Type::TypeMember(id) => self.get(*id).bound_hi.clone(),
            _ => None,
        }
    }

    /// Whether an upper bound has `cls` above it: the class itself, one of its
    /// descendants, any parent of a compound, or -- through another abstract
    /// type -- that type's own bound. Depth-limited, because a bound may name
    /// the parameter it bounds.
    fn bound_reaches(&self, bound: &Type, cls: SymbolId, anc: &mut AncMemo, depth: u32) -> bool {
        if depth > 4 {
            return false;
        }
        match bound {
            Type::Refined { parents, .. } => parents
                .iter()
                .any(|p| self.bound_reaches(p, cls, anc, depth + 1)),
            Type::TypeParam(id) | Type::TypeMember(id) => match self.get(*id).bound_hi.clone() {
                Some(hi) => self.bound_reaches(&hi, cls, anc, depth + 1),
                None => false,
            },
            other => match self.base_arg_class(other) {
                Some(s) => s == cls || self.class_derives_from(s, cls, anc),
                None => false,
            },
        }
    }

    /// Whether `a` is the one of the pair a glb (`most_derived`) or a lub
    /// keeps, judged by class ancestry alone: the arrivals at one base-type
    /// position come from one hierarchy, so a full `is_sub_type` would answer
    /// the same question at the cost of re-entering the typer from inside the
    /// walk every `subst_as_seen_from` runs.
    fn arg_outranks(&self, a: &Type, b: &Type, most_derived: bool, anc: &mut AncMemo) -> bool {
        if a == b {
            return true;
        }
        // `Nothing` is below every type and `Any` above every type, and
        // neither carries the parents that would say so.
        match (a, most_derived) {
            (Type::Nothing, true) | (Type::Any, false) => return true,
            _ => {}
        }
        match (b, most_derived) {
            (Type::Nothing, true) | (Type::Any, false) => return false,
            _ => {}
        }
        // An *abstract* arrival -- a type parameter or a deferred member --
        // carries no parents of its own, so the class ancestry below cannot
        // place it at all and the merge used to keep whichever came first.
        // Its upper bound places it: `C <: LinearSeq[A] with LinearSeqOps[A,
        // CC, C]` says `C` is below `LinearSeq[A]`, so the glb of the two is
        // `C` -- which is what makes `LinearSeqOps.tail`, read through that
        // compound bound, a `C` and not a `LinearSeq[A]`.
        match (self.abstract_bound(a), self.abstract_bound(b)) {
            (Some(_), Some(_)) => return false,
            (Some(ba), None) => {
                let Some(sb) = self.base_arg_class(b) else {
                    return false;
                };
                return most_derived && self.bound_reaches(&ba, sb, anc, 0);
            }
            (None, Some(bb)) => {
                let Some(sa) = self.base_arg_class(a) else {
                    return false;
                };
                return !most_derived && self.bound_reaches(&bb, sa, anc, 0);
            }
            (None, None) => {}
        }
        let (Some(sa), Some(sb)) = (self.base_arg_class(a), self.base_arg_class(b)) else {
            return false;
        };
        if sa == sb {
            return false;
        }
        if most_derived {
            self.class_derives_from(sa, sb, anc)
        } else {
            self.class_derives_from(sb, sa, anc)
        }
    }

    /// Whether `sub` has `sup` above it.
    ///
    /// `class_reaches` first, and `is_ancestor_of` only when it declines to
    /// answer: the two agree on every hierarchy either can read, and
    /// `is_ancestor_of` allocates a hash set per call. Ordering base-type
    /// arrivals asks this question a few times per merged type argument and
    /// the merge runs inside `subst_as_seen_from`, so the allocation showed up
    /// as 4% of the compiler on the standard library.
    fn class_derives_from(&self, sub: SymbolId, sup: SymbolId, anc: &mut AncMemo) -> bool {
        if let Some(&(_, _, r)) = anc.iter().find(|(x, y, _)| *x == sub.0 && *y == sup.0) {
            return r;
        }
        let r = match self.class_reaches(sub, sup) {
            Some(r) => r,
            None => self.is_ancestor_of(sup, sub),
        };
        anc.push((sub.0, sup.0, r));
        r
    }

    /// The class a base-type argument names, for the ancestry test above.
    /// Deliberately not `class_sym_of`: that chases a type parameter to its
    /// bound, and an argument standing for an unknown type is not the bound.
    fn base_arg_class(&self, t: &Type) -> Option<SymbolId> {
        match t {
            Type::Class { sym, .. } | Type::ModuleRef(sym) if !sym.is_none() => Some(*sym),
            Type::Function { .. } => match self.function_class_form(t) {
                Some(Type::Class { sym, .. }) if !sym.is_none() => Some(sym),
                _ => None,
            },
            _ => None,
        }
    }

    pub fn type_of_class(&self, id: SymbolId) -> Type {
        let s = self.get(id);
        match s.kind {
            SymKind::Module | SymKind::ModuleClass => Type::ModuleRef(id),
            _ => Type::Class {
                sym: id,
                args: vec![],
            },
        }
    }

    /// The type `this` has inside `id`: the class applied to *its own* type
    /// parameters.
    ///
    /// `type_of_class` answers with the bare class symbol, which is right for
    /// naming a class but wrong for `this`: in `trait Box[A] { def f = this }`
    /// the raw `Box` carries no arguments, so every later conformance check
    /// has to invent them — and inventing `Any` makes `Box[A]` and `Box[B]`
    /// look interchangeable while making both fail against `Box[A]`.
    pub fn self_type_of_class(&self, id: SymbolId) -> Type {
        let s = self.get(id);
        match s.kind {
            SymKind::Module | SymKind::ModuleClass => Type::ModuleRef(id),
            _ if s.tparams.is_empty() => Type::Class {
                sym: id,
                args: vec![],
            },
            _ => Type::Class {
                sym: id,
                args: s.tparams.iter().map(|t| Type::TypeParam(*t)).collect(),
            },
        }
    }

    /// One of the nine primitive value classes (`scala.Int`, `scala.Unit`, ...).
    ///
    /// Their `jvm_name` records the *box* they erase to (`java/lang/Integer`),
    /// not a class of their own — `scala/Int.class` does not exist. That makes
    /// the field a representation, not an identity: `java.lang.Integer` is a
    /// different Scala type that happens to share the name, so every lookup
    /// that means "the symbol *for* this JVM class" has to skip these.
    pub fn is_primitive_value_class(&self, id: SymbolId) -> bool {
        !id.is_none()
            && [
                self.int_sym,
                self.long_sym,
                self.float_sym,
                self.double_sym,
                self.char_sym,
                self.boolean_sym,
                self.byte_sym,
                self.short_sym,
                self.unit_sym,
            ]
            .contains(&id)
    }

    /// Reassign a symbol's `jvm_name`, keeping `jvm_index` in step.
    ///
    /// The only supported way to change the field after `alloc`: writing it
    /// through `get_mut` leaves the reverse index pointing at the old name and
    /// `find_by_jvm` would then never find the symbol under its new one.
    pub fn set_jvm_name(&mut self, id: SymbolId, jvm: impl Into<String>) {
        let jvm = jvm.into();
        self.note_mutation();
        let sym = &mut self.symbols[id.0 as usize];
        if sym.jvm_name == jvm {
            return;
        }
        sym.jvm_name = jvm;
        let class_like = sym.is_class_like();
        let name = sym.jvm_name.clone();
        if class_like && !name.is_empty() {
            let idx = self.jvm_index.get_mut();
            // Only symbols already folded in need patching; the lazy pass will
            // pick up the rest with the name they have by then.
            if (id.0 as usize) < idx.upto {
                let slot = idx.map.entry(name).or_default();
                if !slot.contains(&id) {
                    slot.push(id);
                }
            }
        }
    }

    /// The first class-like symbol whose `jvm_name` is `jvm`, ignoring the
    /// primitive value classes (whose `jvm_name` is the box they erase to, not
    /// a class of their own).
    ///
    /// Equivalent to a scan of `symbols` in id order, which is what this
    /// replaced: for slick that scan was ~6% of type checking on its own.
    /// Index entries can be stale (a symbol renamed away from `jvm`), so the
    /// name is re-checked here; entries are never *missing*, which is what
    /// `set_jvm_name` buys.
    pub fn find_class_by_jvm(&self, jvm: &str) -> Option<SymbolId> {
        let mut idx = self.jvm_index.borrow_mut();
        if idx.upto < self.symbols.len() {
            let from = idx.upto;
            for s in &self.symbols[from..] {
                if s.is_class_like() && !s.jvm_name.is_empty() {
                    let slot = idx.map.entry(s.jvm_name.clone()).or_default();
                    if !slot.contains(&s.id) {
                        slot.push(s.id);
                    }
                }
            }
            idx.upto = self.symbols.len();
        }
        idx.map
            .get(jvm)?
            .iter()
            .copied()
            .filter(|&id| {
                self.symbols[id.0 as usize].jvm_name == jvm
                    && !self.is_primitive_value_class(id)
                    // A prelude symbol a source definition has replaced is
                    // not the class of that binary name any more.
                    && !self.prelude_shadowed.contains(&id)
            })
            .min_by_key(|s| s.0)
    }

    /// `class C(val x: T) extends AnyVal` — one ctor param, parent AnyVal.
    pub fn is_value_class(&self, id: SymbolId) -> bool {
        if id.is_none() {
            return false;
        }
        let s = self.get(id);
        if s.kind != SymKind::Class
            || s.flags.contains(Flags::TRAIT)
            || s.flags.contains(Flags::INTERFACE)
            || s.ctor_fields.len() != 1
        {
            return false;
        }
        s.parents.iter().any(|p| {
            matches!(p, Type::AnyVal) || self.class_sym_of(p).is_some_and(|c| c == self.anyval_sym)
        })
    }

    pub fn value_class_getter(&self, id: SymbolId) -> &str {
        self.value_class_getters
            .get(&id)
            .map(String::as_str)
            .unwrap_or_else(|| &self.get(self.get(id).ctor_fields[0]).name)
    }

    pub fn value_class_underlying(&self, id: SymbolId) -> Option<Type> {
        if !self.is_value_class(id) {
            return None;
        }
        let f = self.get(id).ctor_fields[0];
        Some(self.get(f).ty.clone())
    }

    pub fn is_sealed(&self, id: SymbolId) -> bool {
        !id.is_none() && self.get(id).flags.contains(Flags::SEALED)
    }

    /// Concrete leaves of a sealed hierarchy (case classes, objects, non-sealed classes).
    pub fn sealed_leaves(&self, id: SymbolId) -> Vec<SymbolId> {
        let mut out = Vec::new();
        let mut seen = rustc_hash::FxHashSet::default();
        fn rec(
            st: &SymbolTable,
            id: SymbolId,
            out: &mut Vec<SymbolId>,
            seen: &mut rustc_hash::FxHashSet<u32>,
        ) {
            if !seen.insert(id.0) {
                return;
            }
            let children = st.get(id).children.clone();
            if children.is_empty() {
                let s = st.get(id);
                if s.kind == SymKind::Class
                    && (s.flags.contains(Flags::TRAIT) || s.flags.contains(Flags::ABSTRACT))
                    && s.flags.contains(Flags::SEALED)
                {
                    return;
                }
                out.push(id);
                return;
            }
            for c in children {
                let cs = st.get(c);
                if cs.flags.contains(Flags::SEALED)
                    && (cs.flags.contains(Flags::TRAIT)
                        || cs.flags.contains(Flags::ABSTRACT)
                        || cs.kind == SymKind::Class)
                    && !cs.children.is_empty()
                {
                    rec(st, c, out, seen);
                } else {
                    out.push(c);
                }
            }
        }
        rec(self, id, &mut out, &mut seen);
        out
    }

    pub fn enclosing_class_named(&self, from: SymbolId, name: &str) -> Option<SymbolId> {
        let mut cur = from;
        while !cur.is_none() {
            let s = self.get(cur);
            let n = s.name.trim_end_matches('$');
            if n == name && s.is_class_like() {
                return Some(cur);
            }
            cur = s.owner;
        }
        None
    }

    /// Repeated element type from the primary constructor, before its
    /// parameter field is viewed as Seq[T] inside the class.
    pub fn repeated_case_element(&self, class: SymbolId) -> Option<Type> {
        let fields = &self.get(class).ctor_fields;
        let last = *fields.last()?;
        if let Type::Repeated(elem) = &self.get(last).ty {
            return Some((**elem).clone());
        }
        self.get(class).members.iter().find_map(|&id| {
            let ctor = self.get(id);
            if ctor.name != "<init>" || !ctor.params.starts_with(fields) {
                return None;
            }
            if let Type::Method { paramss, .. } = &ctor.ty {
                if let Some(Type::Repeated(elem)) = paramss.iter().flatten().nth(fields.len() - 1) {
                    return Some((**elem).clone());
                }
            }
            None
        })
    }

    /// Base types of `t` (its parents, transitively), most specific first,
    /// with the owning class's type parameters substituted away.
    /// `t` itself is not included.
    pub fn base_type_seq(&self, t: &Type) -> Vec<Type> {
        let mut out: Vec<Type> = Vec::new();
        let mut seen: Vec<Type> = vec![t.clone()];
        let mut queue: std::collections::VecDeque<Type> = std::collections::VecDeque::new();
        queue.push_back(t.clone());
        let mut guard = 0usize;
        while let Some(cur) = queue.pop_front() {
            guard += 1;
            if guard > 256 {
                break;
            }
            let (sym, args): (SymbolId, &[Type]) = match &cur {
                Type::Class { sym, args } => (*sym, args),
                Type::ModuleRef(s) | Type::ThisType(s) => (*s, &[]),
                // A type parameter's ancestors are its upper bound's, so
                // `lub(S, S2)` for `S <: NoStream` and `S2 <: NoStream` is
                // `NoStream` and not `AnyRef`.
                Type::TypeParam(id) | Type::TypeMember(id) => {
                    if let Some(hi) = &self.get(*id).bound_hi {
                        if !seen.contains(hi) {
                            seen.push(hi.clone());
                            out.push(hi.clone());
                            queue.push_back(hi.clone());
                        }
                    }
                    continue;
                }
                // A compound bound is every one of its parts. The reflect API
                // is written in these -- `type Ident >: Null <: IdentApi with
                // RefTree` -- and stopping here left `Ident` and `Literal`
                // with no common ancestor but `AnyRef`, so `List(anIdent,
                // aLiteral)` came out as `List[AnyRef]` and no `Syntactic*`
                // call would take it.
                Type::Refined { parents, .. } => {
                    for p in parents {
                        if seen.contains(p) {
                            continue;
                        }
                        seen.push(p.clone());
                        out.push(p.clone());
                        queue.push_back(p.clone());
                    }
                    continue;
                }
                _ => continue,
            };
            let s = self.get(sym);
            for p in &s.parents {
                // `self.subst_tparams_cow`, not the free function: a parent
                // written as `Query[E, E#TableElementType, Seq]` only becomes
                // the base type the source asked for once the projection is
                // reduced at `E`'s argument.
                let p = self.subst_tparams_cow(sym, args, p);
                if seen.contains(&*p) {
                    continue;
                }
                seen.push(p.clone().into_owned());
                out.push(p.clone().into_owned());
                queue.push_back(p.into_owned());
            }
        }
        out
    }

    /// Greatest lower bound: the intersection type, reduced when one side
    /// already conforms to the other. Only used to join a contravariant type
    /// argument, where `A with B` is what nsc records too.
    pub fn glb(&self, a: &Type, b: &Type) -> Type {
        if a == b || self.is_sub_type(a, b) {
            return a.clone();
        }
        if self.is_sub_type(b, a) {
            return b.clone();
        }
        Type::Refined {
            parents: vec![a.clone(), b.clone()],
            decls: Vec::new(),
        }
    }

    /// Least upper bound of two types, used for `[B >: A]` inference and for
    /// varargs element types. Walks the parent chain, so
    /// `lub(Circle, Rect) = Shape` for a sealed `Shape` hierarchy.
    pub fn lub(&self, a: &Type, b: &Type) -> Type {
        self.lub_at(a, b, 0)
    }

    /// `lub` with nsc's depth cap.
    ///
    /// Joining the arguments of two applications of the same class recurses
    /// into those arguments, and with recursive generics that never
    /// terminates: jgit's `getAllRefsByPeeledObjectId` gives a
    /// `java.util.Map[ObjectId, java.util.Set[Ref]]`, and the lub of two of
    /// its instantiations grows one `Comparable[…]` layer per step. gitbucket
    /// crashed the compiler with a stack overflow on it (`JGitUtil.scala`),
    /// with no diagnostic at all. nsc bounds the same recursion with
    /// `Depth`/`maxDepth` and answers `Any` when it runs out; so does this.
    fn lub_at(&self, a: &Type, b: &Type, depth: u32) -> Type {
        /// nsc's `LubGlbMargin` is 0 and its starting depth comes from the
        /// operands; a fixed cap is enough here, since a lub that needs more
        /// than this many nested joins is the pathological case, not a real
        /// answer anybody reads.
        const MAX_LUB_DEPTH: u32 = 6;
        let a = a.widen_constant();
        let b = b.widen_constant();
        if a == b {
            return a;
        }
        if a.is_error() || a.is_no_type() || matches!(a, Type::Nothing) {
            return b;
        }
        if b.is_error() || b.is_no_type() || matches!(b, Type::Nothing) {
            return a;
        }
        if self.is_sub_type(&a, &b) {
            return b;
        }
        if self.is_sub_type(&b, &a) {
            return a;
        }
        // Two distinct value types meet at `AnyVal`, not `Any`: nsc's lub of
        // `Int` and `Double` (or `Boolean`) is `AnyVal`, which is what
        // `Map(1 -> 2, 3 -> 4.5)` is a map *to*. (The *weak* lub, `Double`,
        // is the branch join's business -- `numeric_branch_lub` -- not this.)
        let value_type = |t: &Type| {
            matches!(
                t,
                Type::Unit
                    | Type::Boolean
                    | Type::Byte
                    | Type::Short
                    | Type::Char
                    | Type::Int
                    | Type::Long
                    | Type::Float
                    | Type::Double
            )
        };
        if value_type(&a) && value_type(&b) {
            return Type::AnyVal;
        }
        if depth >= MAX_LUB_DEPTH {
            return if self.is_sub_type(&a, &Type::AnyRef) && self.is_sub_type(&b, &Type::AnyRef) {
                Type::AnyRef
            } else {
                Type::Any
            };
        }
        // Same class constructor, differing arguments: join the arguments.
        // A contravariant parameter joins the other way -- the least upper
        // bound of `Act[R, E]` and `Act[R2, E2]` with `Act[+R, -E]` is
        // `Act[R lub R2, E glb E2]`. Giving up on the whole class because one
        // parameter is contravariant used to send `lub(Act[…], Act[…])` all
        // the way up to `AnyRef`, which is where `Vector(this, a)` got its
        // `Vector[AnyRef]` element type.
        if let (Type::Class { sym: s1, args: a1 }, Type::Class { sym: s2, args: a2 }) = (&a, &b) {
            if s1 == s2 && !a1.is_empty() && a1.len() == a2.len() {
                let tparams = self.get(*s1).tparams.clone();
                let joined: Vec<Type> = a1
                    .iter()
                    .zip(a2.iter())
                    .enumerate()
                    .map(|(i, (x, y))| {
                        let flags = tparams
                            .get(i)
                            .map(|&tp| self.get(tp).flags)
                            .unwrap_or(Flags::EMPTY);
                        if flags.contains(Flags::CONTRAVARIANT) {
                            self.glb(x, y)
                        } else if flags.contains(Flags::COVARIANT) || x == y {
                            self.lub_at(x, y, depth + 1)
                        } else {
                            // An *invariant* parameter admits neither argument
                            // in place of the other, so joining them is not a
                            // type either side conforms to: nsc's lub of
                            // `SBT[Boolean]` and `SBT[Int]` is the existential
                            // `SBT[_ >: Int with Boolean <: AnyVal]`, and
                            // returning `SBT[AnyVal]` made `Seq(boolT, intT)`
                            // inapplicable to `Seq(elems: A*)`.
                            Type::BoundedWildcard {
                                lo: None,
                                hi: Some(Box::new(self.lub_at(x, y, depth + 1))),
                            }
                        }
                    })
                    .collect();
                return Type::Class {
                    sym: *s1,
                    args: joined,
                };
            }
        }
        // `FunctionN[-T1, …, -Tn, +R]` is a class like any other, but it has
        // its own `Type` variant and so never reached the arm above: the lub of
        // `String => Timestamp` and `String => String` walked the base type
        // sequence and answered `AnyRef`. slick's SQLite model builder holds a
        // `Seq` of exactly such mixed converters, and `convertors.iterator.map(fn
        // => Try(fn(v2)))` then said `value apply is not a member of AnyRef`.
        if let (
            Type::Function {
                params: p1,
                ret: r1,
            },
            Type::Function {
                params: p2,
                ret: r2,
            },
        ) = (&a, &b)
        {
            if p1.len() == p2.len() {
                return Type::Function {
                    params: p1
                        .iter()
                        .zip(p2.iter())
                        .map(|(x, y)| self.glb(x, y))
                        .collect(),
                    ret: Box::new(self.lub_at(r1, r2, depth + 1)),
                };
            }
        }
        // Not just `a`'s ancestors: `None` (`<: Option[Nothing]` only) paired
        // with `Some[Boolean]` (`<: Option[Boolean]`) has no match walking
        // only `a`'s chain (`Some[Boolean] <: Option[Nothing]` is false, since
        // `Boolean` is not `<: Nothing`), but walking `b`'s chain finds
        // `Option[Boolean]`, which *does* accept `a` (`Nothing <: Boolean`).
        // A real LUB would also *join* partial candidates from both sides
        // (nsc's answer here is `Option[X] with Product with Serializable`);
        // this version picks one of them, which covers the common "singleton
        // case object vs. parameterized case class" pattern, since one side's
        // own instantiation is precise enough already.
        //
        // The entry that stops the walk may be the *right class at the wrong
        // arguments*: `None`'s sequence reaches `Option[Nothing]`, which
        // `Some[X]` does not conform to, and walking past it lands on whatever
        // `Option`'s own parents are. `scala/Option`'s classfile says
        // `implements scala.Product`, so as soon as anything in a run had made
        // that parent visible, `lub(None, Some(x))` -- which nothing in a
        // small program could get wrong -- answered `Product` in a large one:
        // slick's `PositionedResult.nextBlobOption()` is
        // `if (rs.wasNull) None else Some(r)`, and `nextBlobOption()
        // getOrElse (…)` was `value getOrElse is not a member of Product`
        // (`agent/tail1` / `mismatch10` / `mismatch11` / `tail3` each recorded
        // this as irreproducible outside the full 184-file slick run; the
        // state it depends on is the library's, not slick's). So when the two
        // sequences meet at the same class with different arguments, the
        // arguments are joined and the walk stops there.
        //
        // A type is at the head of its own base type sequence (SLS 3.5.2), and
        // leaving it out is the same failure one step earlier: `lub(Some[X],
        // Option[Y])` never saw `Option` on the second side at all, walked
        // past `Option[X]` and answered `Product` again.
        let with_self = |t: &Type| {
            let mut v = vec![t.clone()];
            v.extend(self.base_type_seq(t));
            v
        };
        let b_seq = with_self(&b);
        for cand in with_self(&a) {
            if matches!(cand, Type::Any | Type::AnyRef | Type::AnyVal) {
                continue;
            }
            if self.is_sub_type(&b, &cand) {
                return cand;
            }
            let Type::Class { sym, args } = &cand else {
                continue;
            };
            if args.is_empty() {
                continue;
            }
            let same = b_seq.iter().find(|t| {
                matches!(t, Type::Class { sym: s2, args: a2 }
                         if s2 == sym && a2.len() == args.len())
            });
            if let Some(other) = same {
                // Both are `Type::Class` at the same symbol, so this hits the
                // argument-joining arm above and terminates.
                let joined = self.lub_at(&cand, other, depth + 1);
                if self.is_sub_type(&a, &joined) && self.is_sub_type(&b, &joined) {
                    return joined;
                }
            }
        }
        for cand in b_seq {
            if matches!(cand, Type::Any | Type::AnyRef | Type::AnyVal) {
                continue;
            }
            if self.is_sub_type(&a, &cand) {
                return cand;
            }
        }
        if self.is_sub_type(&a, &Type::AnyRef) && self.is_sub_type(&b, &Type::AnyRef) {
            Type::AnyRef
        } else {
            Type::Any
        }
    }

    /// SLS 6.26.1: an `Int` literal in range converts to `Byte`, `Short` or
    /// `Char`. This is *narrowing*, not conformance -- overload resolution
    /// only falls back on it, so `sb.append(42)` still picks `append(Int)`.
    pub fn narrows_to(&self, from: &Type, to: &Type) -> bool {
        let Type::Constant(scala_rs_parser::Lit::Int(v)) = from else {
            return false;
        };
        match to {
            Type::Byte => (-128..=127).contains(v),
            Type::Short => (-32768..=32767).contains(v),
            Type::Char => (0..=65535).contains(v),
            _ => false,
        }
    }

    /// Can the parent walk in [`SymbolTable::is_sub_type`] reach class
    /// `target` from class `start`?
    ///
    /// `Some(false)` is a promise that it cannot, and is the only answer worth
    /// having: the caller then skips the walk. `Some(true)` means the symbol
    /// appears somewhere above `start` and the walk has to run for real, since
    /// only it can decide the type arguments. `None` means the hierarchy holds
    /// a parent this cannot model and nothing may be concluded.
    ///
    /// Deliberately an over-approximation of what the real walk visits: it
    /// ignores type substitution (which never changes a parent's *class*) and
    /// it does not re-apply the rewrites `is_sub_type` performs on its way in
    /// (`Array[T]` written as `Class`, for one), so it can only ever claim
    /// that more is reachable, never less.
    ///
    /// Every parent form the real walk treats specially is a `None` here, so
    /// the promise holds for exactly two shapes: a class parent that is not a
    /// `FunctionN` in class clothing (`is_sub_type` turns those into the
    /// structural function type and leaves this walk's world), and `AnyRef` /
    /// `Any` / `AnyVal`, which the real walk answers `false` for against any
    /// class and which are where most hierarchies end.
    pub(crate) fn class_reaches(&self, start: SymbolId, target: SymbolId) -> Option<bool> {
        // Hierarchies are tens of nodes, so a scanned `Vec` beats a hash set.
        let mut seen: Vec<u32> = Vec::with_capacity(32);
        let mut work: Vec<SymbolId> = Vec::with_capacity(16);
        seen.push(start.0);
        work.push(start);
        while let Some(c) = work.pop() {
            for p in &self.get(c).parents {
                match p {
                    Type::Class { sym, args } => {
                        if *sym == target {
                            return Some(true);
                        }
                        if self.is_function_class_shape(*sym, args) {
                            return None;
                        }
                        if !seen.contains(&sym.0) {
                            seen.push(sym.0);
                            work.push(*sym);
                        }
                    }
                    // `is_sub_type` has no arm for these against a class, so
                    // the real walk stops here too.
                    Type::AnyRef | Type::Any | Type::AnyVal => {}
                    _ => return None,
                }
            }
        }
        Some(false)
    }

    /// Run `f`, a fan-out over `a`'s parents asking whether any of them is
    /// under `b`, with the bookkeeping that makes that walk terminate and stay
    /// polynomial.
    ///
    /// The walk used to be bounded by [`enter_depth`] alone. **A depth bound
    /// bounds the depth of the recursion tree, not its size.** With two parents
    /// per node the tree is `2^depth`, and `any()` short-circuits only on
    /// `true` -- the blow-up case is a `false`, which is the answer overload
    /// resolution and implicit search ask for most of the time. Two shapes hit
    /// it, and only the first needs a cycle:
    ///
    /// * A cyclic `extends` graph. `linearize` grew its own guard for this; the
    ///   cycle is diagnosed and the closing edge replaced by `Type::Error`, so
    ///   a *reported* cycle no longer reaches here. This guard is what covers
    ///   the ones that do not (a `ModuleRef` parent, a cycle closed after that
    ///   check, or any parent list this compiler builds wrongly).
    ///
    /// * **A perfectly legal, acyclic hierarchy.** Ten levels of diamonds is
    ///   `2^10` distinct paths to the top and this walk took every one of them.
    ///   26 levels of
    ///
    ///   ```text
    ///   trait A(n) extends A(n-1) with B(n-1)
    ///   trait B(n) extends A(n-1) with B(n-1)
    ///   ```
    ///
    ///   took 74 s to answer one `A26 <: Double`, doubling with every level
    ///   added, where scalac 2.13.16 compiles the same file in 1.9 s. Depth
    ///   there is 26 -- the bound of 200 never fired, and no cycle check could
    ///   have helped, because there is no cycle.
    ///
    /// So a `seen` set keyed on the recursion path is not enough: the second
    /// shape re-reaches the *same* question down a different path, not the same
    /// one. What that needs is the memo. The two together are the standard
    /// least-fixed-point treatment, and are what [`crate::lin`] carries:
    ///
    /// * A question already on `path` is re-entrant, and is answered `false`.
    ///   There is no finite derivation of `a <: b` that needs `a <: b`, so
    ///   `false` is the least fixed point rather than a guess, and every `true`
    ///   reachable without the cycle survives.
    /// * A question already in `memo` is answered from it, which is what turns
    ///   `2^n` into one visit per distinct question.
    /// * A result computed while anything under it truncated at `path` is
    ///   **not** memoised: that answer depends on where the walk came from, and
    ///   caching it would leak one caller's truncation into another's answer.
    ///
    /// Keyed on the whole question, never on the bare symbol: a legitimate walk
    /// revisits a class at different type arguments, and `List[Int]` and
    /// `List[String]` are different questions with different answers.
    ///
    /// [`enter_depth`] stays as the backstop it always was. `subst_tparams_cow`
    /// can *grow* a type argument (`F[F[A]]`), so the set of distinct questions
    /// is not guaranteed finite, and neither the path nor the memo would bound
    /// a walk that keeps inventing new ones.
    fn walk_parents(&self, a: &Type, b: &Type, f: impl FnOnce() -> bool) -> bool {
        let Some(_depth) = enter_depth() else {
            return false;
        };
        let (tracked, answer) = SUBTYPE_WALK.with(|w| {
            let mut w = w.borrow_mut();
            w.depth += 1;
            w.steps = w.steps.saturating_add(1);
            if w.steps <= SUBTYPE_MEMO_AFTER {
                return (false, None);
            }
            if w.path.iter().any(|(x, y)| x == a && y == b) {
                w.truncations += 1;
                return (false, Some(false));
            }
            if let Some(&(_, _, r)) = w.memo.iter().find(|(x, y, _)| x == a && y == b) {
                return (false, Some(r));
            }
            w.path.push((a.clone(), b.clone()));
            (true, None)
        });
        // Held across `f`, which recurses: it pops this walk and, when the
        // outermost one returns, empties the memo.
        let _walk = WalkGuard { tracked };
        if let Some(r) = answer {
            return r;
        }
        let before = SUBTYPE_WALK.with(|w| w.borrow().truncations);
        let r = f();
        if tracked {
            SUBTYPE_WALK.with(|w| {
                let mut w = w.borrow_mut();
                if w.truncations == before {
                    w.memo.push((a.clone(), b.clone(), r));
                }
            });
        }
        r
    }

    pub fn is_sub_type(&self, a: &Type, b: &Type) -> bool {
        if matches!(a, Type::JavaObject) {
            return self.is_sub_type(&Type::AnyRef, b);
        }
        if matches!(b, Type::JavaObject) {
            return self.is_sub_type(a, &Type::AnyRef);
        }
        if a == b {
            return true;
        }
        // `A#B` carries what `A` settles as a refinement so member reads can
        // see it, but it *is* `B`: the same class reached by an alias
        // (`type Session = JdbcSessionDef`) carries nothing, and slick passes
        // the two to each other constantly. Constraining either direction
        // would invent errors nsc does not have.
        // The exception: two views of the *same* class that both carry a
        // prefix are compared prefix first (nsc's `isSubPre`) -- `a.In` is
        // not a `b.In`, and `Outer#In` is not an `a.In`, while `a.In` is an
        // `Outer#In`. See `prefix.rs`.
        if let (Some(p1), Some(p2)) = (crate::prefix::view_prefix(a), crate::prefix::view_prefix(b))
        {
            let (ca, cb) = (crate::prefix::strip_view(a), crate::prefix::strip_view(b));
            if let (Type::Class { sym: s1, .. }, Type::Class { sym: s2, .. }) = (ca, cb) {
                if s1 == s2 {
                    return self.prefix_conforms(p1, p2) && self.is_sub_type(ca, cb);
                }
            }
        }
        if let Some(p) = Self::as_seen_from_view(a) {
            if self.is_sub_type(p, b) {
                return true;
            }
            // The parents of an inner class are written in the enclosing
            // class's vocabulary; only the prefix instantiates them.
            return match crate::prefix::view_prefix(a) {
                Some(pre) => self.prefixed_parents_conform(pre, p, b),
                None => false,
            };
        }
        if let Some(p) = Self::as_seen_from_view(b) {
            // A bare class against an inner class behind a prefix: `new S1
            // {}` inside `P` against `p.S1`. The parents decide, each with
            // the prefix it was written with (`parent_prefixes`): `P.this.S1`
            // is not a `p.S1`, while a parent written `extends o.In` is an
            // `o.In`, and one this compiler has no prefix for conforms as
            // before.
            if crate::prefix::view_prefix(b).is_some() {
                if let Type::Class { sym: s1, args: a1 } = a {
                    if !s1.is_none() && self.class_sym_of(p) != Some(*s1) {
                        return self.walk_parents(a, b, || {
                            let child = self.get(*s1);
                            child.parents.iter().enumerate().any(|(i, q)| {
                                let q = self.subst_tparams_cow(*s1, a1, q);
                                match self.parent_prefixes.get(&(s1.0, i)) {
                                    Some(pre) => {
                                        let qq =
                                            crate::prefix::with_prefix((*q).clone(), pre.clone());
                                        self.is_sub_type(&qq, b)
                                    }
                                    None => self.is_sub_type(&q, b),
                                }
                            })
                        });
                    }
                }
            }
            return self.is_sub_type(a, p);
        }
        // `Array[T]` has two spellings: `Type::Array` from source, and
        // `Class { sym: array_sym }` from a classfile signature or from
        // substituting `Array` for a `C[_]` parameter. They are the same type.
        if let Some(n) = self.array_class_form(a) {
            return self.is_sub_type(&n, b);
        }
        if let Some(n) = self.array_class_form(b) {
            return self.is_sub_type(a, &n);
        }
        // An alias type member stands for its right-hand side on either side of
        // `<:`. This has to happen before the arms below, because the `Class`
        // and `Applied` arms match without ever looking at `b`.
        if matches!(a, Type::TypeMember(_)) {
            let d = self.dealias(a);
            if d != *a {
                return self.is_sub_type(&d, b);
            }
        }
        if matches!(b, Type::TypeMember(_)) {
            let d = self.dealias(b);
            if d != *b {
                return self.is_sub_type(a, &d);
            }
        }
        // A higher-kinded alias is a type lambda, and `dealias` deliberately
        // leaves it folded because its body only means anything once applied.
        // Two spellings of the same lambda therefore never compare equal by
        // symbol, so compare the bodies instead. See `hk_alias_sub_type`.
        if let Some(r) = self.hk_alias_sub_type(a, b) {
            return r;
        }
        // A path-dependent member (`p.T`) and the bare declaration it stands
        // for (`T`) conform in both directions: the bare declaration is what
        // this compiler still produces everywhere the prefix was not tracked,
        // and refusing it would invent errors nsc does not have. Two members
        // that *both* carry a path are deliberately left to the arms below --
        // `p.T` and `q.T` are different types, which is the whole point.
        if !self.path_member_decl.is_empty() {
            let (pa, pb) = (self.mentions_path_member(a), self.mentions_path_member(b));
            if pa != pb {
                return if pa {
                    self.is_sub_type(&self.drop_path_members(a), b)
                } else {
                    self.is_sub_type(a, &self.drop_path_members(b))
                };
            }
        }
        // An abstract projection (`E#T`, `E` still a type parameter) and the
        // bare declaration it projects conform in both directions, for the
        // same reason a path member does: the bare declaration is what every
        // route that does not carry a prefix still produces -- including the
        // erased generic signature of the very class the projection was read
        // out of -- and refusing it would invent errors, not find them. Two
        // projections through *different* prefixes are left to the arms
        // below and stay distinct, which is what makes the reduction sound.
        if !self.abs_projection_of.is_empty() {
            let (pa, pb) = (
                self.mentions_abs_projection(a),
                self.mentions_abs_projection(b),
            );
            if pa != pb {
                return if pa {
                    self.is_sub_type(&self.drop_abs_projections(a), b)
                } else {
                    self.is_sub_type(a, &self.drop_abs_projections(b))
                };
            }
        }
        // A method type parameter the current `case` has bounded (GADT
        // refinement, `gadt_bounds`): `T` at `Int..Int` inside `case I(i)`
        // takes an `Int` (`i` conforms to the result type `T`) and *is* an
        // `Int` (`ord: Ordering[T]` conforms to the invariant `Ordering[Int]`).
        if !self.gadt_bounds.is_empty() {
            if let Type::TypeParam(id) = b {
                if let Some(lo) = self.gadt_lo(*id) {
                    if self.is_sub_type(a, lo) {
                        return true;
                    }
                }
            }
            if let Type::TypeParam(id) = a {
                if let Some(hi) = self.gadt_hi(*id) {
                    if self.is_sub_type(hi, b) {
                        return true;
                    }
                }
            }
        }
        // An abstract type on the *right* is at least its lower bound:
        // `def f[E, O >: E](x: E): O = x` is legal, and so is every
        // `ShapedValue[_ <: E, U]` where a `ShapedValue[_ <: O, U]` is wanted.
        // Only the bound can settle this -- every arm below either matches on
        // `a` alone or asks for the two to be the same parameter.
        if let Type::TypeParam(id) | Type::TypeMember(id) = b {
            if let Some(lo) = &self.get(*id).bound_lo {
                if !matches!(lo, Type::Nothing) {
                    if let Some(_g) = enter_bound(*id) {
                        if self.is_sub_type(a, lo) {
                            return true;
                        }
                    }
                }
            }
        }
        // One class is under another only if the second one's *symbol* is
        // somewhere in the first one's parent DAG. That question needs no type
        // arguments, so it can be answered by walking symbols with a visited
        // set -- linear -- while the walk below substitutes the arguments at
        // every edge and revisits every diamond, which is what makes a "no"
        // expensive. Implicit search asks far more questions than it accepts,
        // so the "no" is the answer worth making cheap.
        if let (Type::Class { sym: s1, args: a1 }, Type::Class { sym: s2, .. }) = (a, b) {
            if s1 != s2
                && !self.is_function_class_shape(*s1, a1)
                && self.class_reaches(*s1, *s2) == Some(false)
            {
                return false;
            }
        }
        match (a, b) {
            (Type::Error, _) | (_, Type::Error) => true,
            (Type::Nothing, _) => true,
            (_, Type::Any) => true,
            (Type::Class { sym, .. }, Type::AnyRef | Type::AnyVal)
                if *sym == self.singleton_sym => false,
            (Type::Constant(lit), Type::Class { sym, .. })
                if *sym == self.singleton_sym && !matches!(lit, scala_rs_parser::Lit::Unit) => true,
            (Type::SingleType { .. } | Type::ThisType(_) | Type::ModuleRef(_),
                Type::Class { sym, .. }) if *sym == self.singleton_sym => true,
            (Type::Constant(a), Type::Constant(b)) => a == b,
            (Type::Constant(a), b) => self.is_sub_type(&Type::lit_underlying(a), b),
            (
                Type::Null,
                Type::AnyRef
                | Type::String
                | Type::Array(_)
                | Type::Class { .. }
                | Type::ModuleRef(_)
                | Type::Refined { .. }
                | Type::ThisType(_)
                | Type::SingleType { .. }
                | Type::Annotated { .. }
                // A function and a tuple are reference types too: `val f: Int
                // => String = null` is legal Scala.
                | Type::Function { .. }
                | Type::Tuple(_)
                | Type::Applied { .. }
                | Type::Named { .. },
            ) => true,
            (
                Type::Int
                | Type::Long
                | Type::Double
                | Type::Boolean
                | Type::Byte
                | Type::Short
                | Type::Unit
                | Type::Char
                | Type::Float,
                Type::AnyVal,
            ) => true,
            (
                Type::String
                | Type::Array(_)
                | Type::Class { .. }
                | Type::ModuleRef(_)
                | Type::Function { .. }
                | Type::Refined { .. }
                | Type::ThisType(_)
                | Type::SingleType { .. }
                | Type::Annotated { .. }
                | Type::Applied { .. },
                Type::AnyRef,
            ) => true,
            (Type::Class { sym: s1, args: a1 }, Type::Class { sym: s2, args: a2 }) if s1 == s2 => {
                if a1.is_empty() || a2.is_empty() {
                    true
                } else if a1.len() == a2.len() {
                    let tparams = &self.get(*s1).tparams;
                    a1.iter().zip(a2.iter()).enumerate().all(|(i, (x, y))| {
                        let flags = tparams
                            .get(i)
                            .map(|&tp| self.get(tp).flags)
                            .unwrap_or(Flags::EMPTY);
                        // `C[_]` for `class C[+F <: Option[Node]]` is
                        // `C[_$1] forSome { type _$1 <: Option[Node] }`: the
                        // parameter's own bound is the wildcard's. Without it
                        // `C[_] <: C[Option[Node]]` asked `_ <: Option[Node]`
                        // with nothing to answer from and said no, and slick's
                        // `case (c: Comprehension[?], _) => fix(ch, Some(c))`
                        // reported `no matching overload … with arguments
                        // (Node, Some[Comprehension[_]])`. Only the *left*
                        // side is widened: a wildcard on the right already
                        // contains everything.
                        let bounded;
                        let x = match (x, tparams.get(i)) {
                            (Type::Wildcard, Some(&tp)) => match self.get(tp).bound_hi.clone() {
                                Some(hi) => {
                                    bounded = Type::BoundedWildcard {
                                        lo: None,
                                        hi: Some(Box::new(hi)),
                                    };
                                    &bounded
                                }
                                None => x,
                            },
                            _ => x,
                        };
                        if flags.contains(Flags::CONTRAVARIANT) {
                            if is_wildcard_arg(y) {
                                // A wildcard argument stands for *some* type,
                                // so it contains the other whatever the
                                // parameter's variance says: slick's
                                // `SetParameter[T1]` is a `SetParameter[_]`
                                // even though `SetParameter[-T]` is
                                // contravariant. Reading the wildcard as a
                                // type to flip against rejected every
                                // `SetTupleParameter(c1, c2, …)`.
                                self.is_sub_type(x, y)
                            } else {
                                self.is_sub_type(y, x)
                            }
                        } else if flags.contains(Flags::COVARIANT) {
                            self.is_sub_type(x, y)
                        } else if is_wildcard_arg(y) {
                            // An invariant parameter still *contains* a
                            // wildcard: `List[Byte]` is a
                            // `Collection[_ <: Number]`.
                            //
                            // Only on the *right*. A wildcard on the left is
                            // an existential -- `Box[_ <: Unit]` is
                            // `Box[t] forSome { type t <: Unit }` -- and an
                            // invariant parameter admits it in place of
                            // `Box[Unit]` only if `t` *is* `Unit`, which the
                            // wildcard is precisely not saying. Reading it as
                            // containment in both directions accepted
                            // `val bad: Box[Unit] = x` for an `x` inferred as
                            // `Box[_ <: Unit]`, which nsc rejects with a note
                            // about `Box` being invariant.
                            self.is_sub_type(x, y)
                        } else if is_wildcard_arg(x) {
                            false
                        } else {
                            // Invariant: `A[Int]` is not an `A[Any]`.
                            self.is_sub_type(x, y) && self.is_sub_type(y, x)
                        }
                    })
                } else {
                    false
                }
            }
            // Annotations are erased for conformance: `Node` is a
            // `Node @uncheckedVariance`. nsc strips both sides in `firstTry`,
            // before any of the `TypeRef` cases, and this has to do the same:
            // the `Applied` arms below match on *one* side and every `other`,
            // so with these two arms sitting after them `CC[CC[A]]` and
            // `CC[CC[A] @uncheckedVariance]` never reached the rule that says
            // the annotation is a spelling. `agent/basetypemeet` established
            // that reading for base type arguments; conformance is the other
            // half of it, and `Factory.scala`'s `fill`/`tabulate` ladder wants
            // it in both directions.
            //
            // Above the `Applied` arms but *below* the ones that name
            // `Annotated` in a pattern list -- `Null <: T @ann` and
            // `T @ann <: AnyRef` are answered there for every `T`, including
            // the value classes stripping would then reject.
            (a, Type::Annotated { tpe, .. }) => self.is_sub_type(a, tpe),
            (Type::Annotated { tpe, .. }, b) => self.is_sub_type(tpe, b),
            // A wildcard stands for *some* type, so anything is under it --
            // including the application of an abstract type constructor.
            // `Query[B, BU, C]` inherits `Rep[C[BU]]`, and slick's
            // `StreamingExecutable.apply[T <: Rep[_], TU, EU]` asks whether
            // that is a `Rep[_]`; the invariant-argument rule then asks
            // `C[BU] <: _`. These two arms have to precede the `Applied`
            // catch-all below, which matches every `other` and answered "no"
            // for an `Applied` whose constructor is a type *parameter* (it
            // only knew how to follow a `TypeMember`'s bound).
            (Type::Applied { .. }, Type::Wildcard) => true,
            (Type::Applied { .. }, Type::BoundedWildcard { hi, .. }) => match hi {
                Some(h) => self.is_sub_type(a, h),
                None => true,
            },
            (Type::Applied { ctor: c1, args: a1 }, Type::Applied { ctor: c2, args: a2 })
                if a1.len() == a2.len() =>
            {
                // An abstract type constructor *parameter* (`F[_, _]`) is
                // applied at exactly the variance its own parameters declare:
                // `F[_]` is invariant, so `F[(Int, Int)]` is not an
                // `F[AnyRef]` -- scalac reports the mismatch with a note about
                // `F` being invariant. Reading every argument covariantly let
                // cats' `compose(swap, compose(first(fa), swap))` at a wrong
                // declared result through once the call's `B` had been lubbed
                // to `AnyRef`. Other constructor shapes (a partially applied
                // class, a type lambda, a type member) keep the covariant
                // reading they had.
                let variances: Option<Vec<Flags>> = match c1.as_ref() {
                    // A wildcard constructor on the right is an expectation
                    // some enclosing call has not decided (`_[Option[_]]`).
                    _ if matches!(c2.as_ref(), Type::Wildcard) => None,
                    Type::TypeParam(id) if self.get(*id).tparams.len() == a1.len() => Some(
                        self.get(*id)
                            .tparams
                            .iter()
                            .map(|tp| self.get(*tp).flags)
                            .collect(),
                    ),
                    _ => None,
                };
                self.is_sub_type(c1, c2)
                    && a1.iter().zip(a2.iter()).enumerate().all(|(i, (x, y))| {
                        let Some(flags) = variances.as_ref().map(|v| v[i]) else {
                            return self.is_sub_type(x, y);
                        };
                        if flags.contains(Flags::CONTRAVARIANT) {
                            if is_wildcard_arg(y) {
                                self.is_sub_type(x, y)
                            } else {
                                self.is_sub_type(y, x)
                            }
                        } else if flags.contains(Flags::COVARIANT)
                            || is_wildcard_arg(y)
                            || is_wildcard_arg(x)
                        {
                            // A wildcard on either side is an argument some
                            // enclosing call has not decided yet: containment,
                            // as before.
                            self.is_sub_type(x, y)
                        } else {
                            self.is_sub_type(x, y) && self.is_sub_type(y, x)
                        }
                    })
            }
            // `_[_]`: an undetermined type constructor applied to arguments,
            // the shape `check_apply` relaxes `G[B]` to while the argument
            // that decides `G` is being typed. nsc's `appliedType(WildcardType,
            // args)` is `WildcardType` itself; here the application is kept so
            // that the arity is still visible, and anything with at least that
            // many type arguments is under it -- partial unification captures
            // the surplus (`St[S, B]` is `_[_]` with `G := St[S, *]`). `Any` and
            // `Nothing` are kind-polymorphic, as in `unifySimple`.
            (other, Type::Applied { ctor, args }) if matches!(**ctor, Type::Wildcard) => {
                match other {
                    Type::Any | Type::Nothing | Type::Wildcard | Type::Null => true,
                    Type::Class { args: oa, .. } | Type::Applied { args: oa, .. } => {
                        oa.len() >= args.len()
                    }
                    Type::Tuple(ts) => ts.len() >= args.len(),
                    Type::Function { params, .. } => params.len() + 1 >= args.len(),
                    Type::Array(_) => args.len() == 1,
                    _ => false,
                }
            }
            (Type::Applied { ctor, args }, other) => {
                let folded = apply_type_ctor((**ctor).clone(), args.clone());
                if let Type::Applied { ctor, .. } = &folded {
                    // `type BaseColumnType[T] = JdbcType[T] & BaseTypedType[T]`
                    // applied to `U` is that intersection, and conforms to
                    // either half. Only an *abstract* member is stuck at its
                    // upper bound.
                    let expanded = self.expand_applied_hk_alias(folded.clone());
                    if expanded != folded {
                        return self.is_sub_type(&expanded, other);
                    }
                    // An applied abstract constructor is at least its own
                    // bound, applied to the same arguments. A higher-kinded
                    // type *parameter* is that too, not only a type member:
                    // `BuildFrom` declares `CC[X, Y] <: MapOps[X, Y, CC, _]`,
                    // so `CC[K0, V0]` *is* a `MapOps[K0, V0, CC, _]` and
                    // `(from: MapOps[K0, V0, CC, _])` is the ascription of a
                    // value to its own declared bound. nsc has no such split:
                    // `isHKSubType` falls through to `isSubType2`'s
                    // `AbstractTypeRef` case, which reads `sym.info.bounds.hi`
                    // for a `PolyType`-shaped abstract symbol whatever kind of
                    // symbol it is.
                    if let Type::TypeMember(id) | Type::TypeParam(id) = ctor.as_ref() {
                        if let Some(hi) = self.get(*id).bound_hi.clone() {
                            // The bound is written in the member's *own*
                            // parameters: `type CT[T] <: TT[T]` applied to `U`
                            // is bounded by `TT[U]`, not `TT[T]`. Comparing
                            // the un-substituted bound made every applied
                            // abstract member fail its own bound, which is how
                            // slick's `implicitly[BaseColumnType[U]]` (whose
                            // only candidate is the context bound's own
                            // evidence) reported "could not find implicit".
                            let args = match &folded {
                                Type::Applied { args, .. } => args.clone(),
                                _ => Vec::new(),
                            };
                            // `CC[X, Y] <: MapOps[X, Y, CC, _]` mentions `CC`
                            // again, so reading the bound has to be guarded the
                            // way every other bound arm in this function is --
                            // an F-bounded constructor would otherwise expand
                            // its own bound forever.
                            if let Some(_g) = enter_bound(*id) {
                                let hi = self.subst_tparams(*id, &args, &hi);
                                return self.is_sub_type(&hi, other);
                            }
                        }
                    }
                    false
                } else {
                    self.is_sub_type(&folded, other)
                }
            }
            (other, Type::Applied { ctor, args }) => {
                let folded = apply_type_ctor((**ctor).clone(), args.clone());
                if matches!(folded, Type::Applied { .. }) {
                    let expanded = self.expand_applied_hk_alias(folded.clone());
                    if expanded != folded {
                        return self.is_sub_type(other, &expanded);
                    }
                    // An *abstract* member applied to arguments has no
                    // right-hand side to expand, and nothing on the right can
                    // decide the question. nsc then falls through to the rules
                    // for the left side, which is how a compound type conforms
                    // through one of its own parents:
                    // `SqlStreamingAction[R, T, E] with PA[R, Streaming[T], E]`
                    // is a `PA[R, Streaming[T], E]`.
                    match other {
                        Type::Refined { parents, .. } => {
                            parents.iter().any(|p| self.is_sub_type(p, b))
                        }
                        _ => false,
                    }
                } else {
                    self.is_sub_type(other, &folded)
                }
            }
            // Before the Class-parent walk: that arm matches every Class and
            // would otherwise hide `Tuple2[A, B] <: (A, B)` / the reverse.
            (Type::Tuple(a), Type::Tuple(b)) if a.len() == b.len() => {
                a.iter().zip(b.iter()).all(|(x, y)| self.is_sub_type(x, y))
            }
            (Type::Tuple(ts), Type::Class { sym, args })
                if self.is_tuple_arity(*sym, ts.len())
                    && (args.is_empty() || args.len() == ts.len()) =>
            {
                args.is_empty()
                    || ts
                        .iter()
                        .zip(args.iter())
                        .all(|(x, y)| self.is_sub_type(x, y))
            }
            (Type::Class { sym, args }, Type::Tuple(ts))
                if self.is_tuple_arity(*sym, ts.len())
                    && (args.is_empty() || args.len() == ts.len()) =>
            {
                args.is_empty()
                    || args
                        .iter()
                        .zip(ts.iter())
                        .all(|(x, y)| self.is_sub_type(x, y))
            }
            (a, Type::Refined { parents, decls }) => {
                parents.iter().all(|p| self.is_sub_type(a, p))
                    && self.conforms_to_refinement(a, decls)
            }
            // A module has exactly one value, so `Nil.type` and the type of
            // the `Nil` object are the same type -- `Some(Nil)` is a
            // `Some[Nil.type]`. Only for modules: `x.type` for an ordinary
            // `val x: T` is strictly smaller than `T`. Before the
            // Class-parent walk for the same reason as the arms below.
            (a, Type::SingleType { sym, .. })
                if matches!(self.get(*sym).kind, SymKind::Module | SymKind::ModuleClass) =>
            {
                let t = &self.get(*sym).ty;
                !t.is_no_type()
                    && !matches!(t, Type::SingleType { sym: s2, .. } if s2 == sym)
                    && self.is_sub_type(a, t)
            }
            // Wildcards before the Class-parent walk: that arm matches every Class
            // and would otherwise treat `Byte <: List[_ <: Byte]` as "walk Byte's
            // parents" instead of the bound.
            (_, Type::Wildcard) => true,
            (Type::Wildcard, Type::AnyRef | Type::AnyVal) => true,
            (a, Type::BoundedWildcard { hi, .. }) => match hi {
                Some(h) => self.is_sub_type(a, h),
                None => true,
            },
            (Type::BoundedWildcard { hi, .. }, b) => match hi {
                Some(h) => self.is_sub_type(h, b),
                None => matches!(b, Type::Any | Type::AnyRef | Type::Wildcard),
            },
            // `Type::String` is `java.lang.String`, which is not a leaf: it
            // implements `CharSequence`, `Comparable<String>` and
            // `Serializable` (`prelude_strhier`). Without this walk every JDK
            // overload taking a `CharSequence` was inapplicable to a `String`.
            (Type::String, b) => self.walk_parents(a, b, || {
                let parents = &self.get(self.string_sym).parents;
                parents.iter().any(|p| self.is_sub_type(p, b))
            }),
            (Type::Class { sym: s1, args: a1 }, b) => {
                // `scala.FunctionN[T1, …, R]` and the structural function type
                // are one and the same type; the prelude writes a parent that
                // *is* a function (`PartialFunction`, `Map`) as the class and
                // everything else as the structural form.
                if let Some(f) = self.function_class_shape(*s1, a1) {
                    return self.is_sub_type(&f, b);
                }
                // A malformed hierarchy (`object B extends B`) would otherwise
                // walk its own parents forever, and a legal one that is merely
                // deep and diamond-shaped would take `2^depth` paths to the
                // top. `walk_parents` is where both are stopped, and its
                // comment says why neither a depth bound nor a set keyed on the
                // bare symbol is enough.
                self.walk_parents(a, b, || {
                    // Borrowed: this is the arm the subtype walk spends most of
                    // its time in, and cloning the parent list and the type
                    // parameters at every node of the DAG dominated its cost.
                    let child = self.get(*s1);
                    child.parents.iter().any(|p| {
                        // `self.`, not the free function: a parent carrying an
                        // abstract projection over `s1`'s own parameters is only
                        // the base type the question is about once the projection
                        // is reduced at `a1` (`SymbolTable::subst_projections`).
                        // The method costs one `is_empty` when no program in this
                        // run has a projection at all.
                        let p = self.subst_tparams_cow(*s1, a1, p);
                        self.is_sub_type(&p, b)
                    })
                })
            }
            // `(A, B)` *is* `Tuple2[A, B]`, so everything it inherits --
            // `Product`, `Serializable`, `Equals`, `Product2[A, B]` -- comes
            // from that class's parents. The tuple-to-tuple arms above have
            // already run; this is the ordinary parent walk for every other
            // right-hand side.
            (Type::Tuple(ts), b) if !ts.is_empty() => {
                let Some(_g) = enter_depth() else {
                    return false;
                };
                match self.class_sym_of(a) {
                    Some(sym) => self.is_sub_type(
                        &Type::Class {
                            sym,
                            args: ts.clone(),
                        },
                        b,
                    ),
                    None => false,
                }
            }
            // `Array` is invariant: scalac rejects an `Array[Int]` where an
            // `Array[Any]` is asked for. A wildcard argument still *contains*
            // the other (`Array[Byte]` is an `Array[_ <: AnyVal]`), same rule
            // as for an invariant class parameter above.
            (Type::Array(x), Type::Array(y)) => {
                // A Java `Object[]` element is nsc's `ObjectTpeJava`, which is
                // the same type as both `Any` and `AnyRef`: scalac passes an
                // `Array[Any]` and an `Array[AnyRef]` to
                // `Arrays.fill(Object[], Object)`, and still rejects an
                // `Array[String]` (the second half below).
                let java_object_twin =
                    |j: &Type, o: &Type| matches!(j, Type::JavaObject) && matches!(o, Type::Any | Type::AnyRef | Type::JavaObject);
                if java_object_twin(x, y) || java_object_twin(y, x) {
                    return true;
                }
                if is_wildcard_arg(x) || is_wildcard_arg(y) {
                    self.is_sub_type(x, y)
                } else {
                    self.is_sub_type(x, y) && self.is_sub_type(y, x)
                }
            }
            (Type::ModuleRef(s), Type::Class { sym, .. }) if s == sym => true,
            (Type::ModuleRef(s), b) => self.walk_parents(a, b, || {
                self.get(*s).parents.iter().any(|p| self.is_sub_type(p, b))
            }),
            (Type::TypeParam(a), Type::TypeParam(b)) if a == b => true,
            (Type::TypeMember(a), Type::TypeMember(b)) if a == b => true,
            (Type::TypeMember(id), b) => {
                if let Some(hi) = &self.get(*id).bound_hi {
                    if let Some(_g) = enter_bound(*id) {
                        if self.is_sub_type(hi, b) {
                            return true;
                        }
                    }
                }
                matches!(b, Type::AnyRef | Type::AnyVal | Type::Any)
            }
            // `def f[A <: Named](x: A)` may use `x` where a `Named` is wanted.
            (Type::TypeParam(id), b) => {
                if let Some(hi) = &self.get(*id).bound_hi {
                    // `A <: Rep[A]` must not expand its own bound again.
                    if let Some(_g) = enter_bound(*id) {
                        if self.is_sub_type(hi, b) {
                            return true;
                        }
                    }
                }
                matches!(b, Type::AnyRef | Type::AnyVal)
            }
            (Type::ThisType(s), b) => {
                if matches!(b, Type::ThisType(t) if t == s) {
                    true
                } else {
                    self.is_sub_type(&self.type_of_class(*s), b)
                }
            }
            (Type::SingleType { sym, prefix }, b) => {
                if matches!(b, Type::SingleType { sym: s2, .. } if s2 == sym) {
                    true
                } else {
                    let t = &self.get(*sym).ty;
                    if t.is_no_type() {
                        self.is_sub_type(prefix, b)
                    } else {
                        self.is_sub_type(t, b)
                    }
                }
            }
            (
                Type::Function {
                    params: p1,
                    ret: r1,
                },
                Type::Function {
                    params: p2,
                    ret: r2,
                },
            ) if p1.len() == p2.len() => {
                p2.iter().zip(p1.iter()).all(|(exp, act)| {
                    // An *unbounded* wildcard parameter on the right stands
                    // for some type with nothing said about it, so it contains
                    // the actual one however the parameter's (contravariant)
                    // variance would flip -- the reading the `Type::Class` arm
                    // above gives `SetParameter[_]` for a contravariant
                    // `SetParameter[-T]`. `Function1[_, _]` is reached
                    // structurally rather than as a `Type::Class`, so it needs
                    // the rule here too: `Promise.scala`'s `def this(xform:
                    // Int, f: _ => _, ec: ExecutionContext)` takes every
                    // `Try[T] => Try[S]`, and flipping against the wildcard
                    // rejected all eleven calls.
                    //
                    // A *bounded* one keeps the flip. `Function1[Any, Unit]` is
                    // a `Function1[_ <: AnyRef, Unit]` -- witness `AnyRef`,
                    // which `Any` accepts contravariantly -- and containment
                    // would ask `Any <: _ <: AnyRef` and say no
                    // (`tests/fixtures/existential_bounds.scala`).
                    if matches!(exp, Type::Wildcard) {
                        true
                    } else {
                        self.is_sub_type(exp, act)
                    }
                }) && self.is_sub_type(r1, r2)
            }
            (Type::Function { .. }, Type::Class { sym, args }) => {
                match self.function_class_shape(*sym, args) {
                    Some(f) => self.is_sub_type(a, &f),
                    None => false,
                }
            }
            (Type::ByName(a), Type::ByName(b)) => self.is_sub_type(a, b),
            (Type::Repeated(a), Type::Repeated(b)) => self.is_sub_type(a, b),
            (Type::Refined { parents, .. }, b) => parents.iter().any(|p| self.is_sub_type(p, b)),
            _ => false,
        }
    }

    /// `scala.FunctionN[T1, …, Tn, R]` read as the structural `(T1, …, Tn) => R`.
    /// `None` for every other class, `PartialFunction` included -- that one
    /// reaches its `Function1` parent through the ordinary walk.
    pub(crate) fn function_class_shape(&self, sym: SymbolId, args: &[Type]) -> Option<Type> {
        if !self.is_function_class_shape(sym, args) {
            return None;
        }
        let n = args.len() - 1;
        Some(Type::Function {
            params: args[..n].to_vec(),
            ret: Box::new(args[n].clone()),
        })
    }

    /// Whether [`SymbolTable::function_class_shape`] would answer `Some`.
    ///
    /// Split out because the parent walks only want the question, and building
    /// the structural function type to throw it away allocated a `Vec` and a
    /// `Box` per class visited.
    pub(crate) fn is_function_class_shape(&self, sym: SymbolId, args: &[Type]) -> bool {
        if args.is_empty() {
            return false;
        }
        let s = self.get(sym);
        let Some(digits) = s.jvm_name.strip_prefix("scala/Function") else {
            return false;
        };
        matches!(digits.parse::<usize>(), Ok(n) if args.len() == n + 1)
    }

    /// The structural `(T1, …, Tn) => R` read back as the class
    /// `scala.FunctionN[T1, …, Tn, R]` -- the inverse of
    /// `function_class_shape`. `class_sym_of` deliberately leaves
    /// `Type::Function` structural (conformance and erasure want it that way),
    /// so the places that need a *class* -- a parent walk, and a member's type
    /// as seen from a prefix -- ask for this form explicitly.
    pub fn function_class_form(&self, ty: &Type) -> Option<Type> {
        let Type::Function { params, ret } = ty else {
            return None;
        };
        let sym = crate::classpath::find_by_jvm(self, &format!("scala/Function{}", params.len()))?;
        let mut args = params.clone();
        args.push((**ret).clone());
        Some(Type::Class { sym, args })
    }

    fn is_tuple_arity(&self, sym: SymbolId, n: usize) -> bool {
        let s = self.get(sym);
        let name = s.name.trim_end_matches('$');
        if name == format!("Tuple{n}") {
            return true;
        }
        let jvm = if s.jvm_name.is_empty() {
            String::new()
        } else {
            s.jvm_name.clone()
        };
        jvm == format!("scala/Tuple{n}")
    }

    /// One refinement declaration, with the symbol table to hand.
    ///
    /// `RefineDecl`'s own `Display` has no table, so every class in it prints
    /// as `#4711` and a higher-kinded member's right-hand side as `tmem#5125`.
    fn display_refine_decl(&self, d: &RefineDecl) -> String {
        match d {
            RefineDecl::Type {
                name,
                rhs,
                tparams,
                lo,
                hi,
            } => {
                let mut s = format!("type {name}");
                // The parameters of a higher-kinded member are the lambda's,
                // and the right-hand side is the lambda itself; print the two
                // together (`type L[a] = List[a]`) rather than a self-reference.
                match rhs.as_ref().and_then(|t| self.lambda_parts(t)) {
                    Some((names, body)) => {
                        s.push_str(&format!(
                            "[{}] = {}",
                            names.join(", "),
                            self.display_type(&body)
                        ));
                        return s;
                    }
                    None => {
                        if *tparams > 0 {
                            s.push_str(&format!("[{}]", vec!["_"; *tparams].join(", ")));
                        }
                    }
                }
                if let Some(t) = lo {
                    s.push_str(&format!(" >: {}", self.display_type(t)));
                }
                if let Some(t) = hi {
                    s.push_str(&format!(" <: {}", self.display_type(t)));
                }
                if let Some(t) = rhs {
                    s.push_str(&format!(" = {}", self.display_type(t)));
                }
                s
            }
            RefineDecl::Def { name, paramss, ret } => {
                let mut s = format!("def {name}");
                for ps in paramss {
                    let ps: Vec<String> = ps.iter().map(|p| self.display_type(p)).collect();
                    s.push_str(&format!("({})", ps.join(", ")));
                }
                s.push_str(&format!(": {}", self.display_type(ret)));
                s
            }
            RefineDecl::Val { name, ty } => format!("val {name}: {}", self.display_type(ty)),
        }
    }

    /// A type lambda, printed the way nsc prints one: `[a]Either[String, a]`.
    ///
    /// Only for a lambda written as a projection out of a refinement --
    /// `refinement_type_member` allocates those with no owner, so the
    /// alternative reading was the useless `<none>.L`. A *named* higher-kinded
    /// alias keeps its name, as it does in nsc.
    fn display_type_lambda(&self, ty: &Type) -> Option<String> {
        let (names, body) = self.lambda_parts(ty)?;
        Some(format!(
            "[{}]{}",
            names.join(", "),
            self.display_type(&body)
        ))
    }

    /// The parameter names and the body of a refinement type lambda, with
    /// whatever it captured already substituted in.
    ///
    /// `None` for anything else, and for a *named* higher-kinded alias, which
    /// keeps its name in a diagnostic as it does in nsc. The members
    /// `refinement_type_member` allocates are the ones with no owner.
    fn lambda_parts(&self, ty: &Type) -> Option<(Vec<String>, Type)> {
        let (id, applied): (SymbolId, &[Type]) = match ty {
            Type::TypeMember(id) => (*id, &[]),
            Type::Applied { ctor, args } => match ctor.as_ref() {
                Type::TypeMember(id) => (*id, args),
                _ => return None,
            },
            _ => return None,
        };
        if !self.get(id).owner.is_none() {
            return None;
        }
        let (params, body) = self.hk_alias(ty)?;
        let names: Vec<String> = params.iter().map(|p| self.get(*p).name.clone()).collect();
        // The captured parameters are the leading ones; substitute what the
        // partial application already fixed before printing the body.
        let body = subst_tparams_slice(&self.get(id).tparams[..applied.len()], applied, body);
        Some((names, body))
    }

    /// `method left`, `class IorT`, `trait Functor` -- what a type parameter
    /// belongs to, for a diagnostic that would otherwise print two different
    /// parameters under the same name. `None` when the owner says nothing
    /// (no symbol, or a package).
    pub(crate) fn tparam_owner_desc(&self, tp: SymbolId) -> Option<String> {
        let owner = self.get(tp).owner;
        if owner.is_none() {
            return None;
        }
        let s = self.get(owner);
        let what = match s.kind {
            SymKind::Method => "method",
            SymKind::Class if s.flags.contains(Flags::TRAIT) => "trait",
            SymKind::Class => "class",
            SymKind::Module | SymKind::ModuleClass => "object",
            SymKind::TypeMember => "type",
            _ => return None,
        };
        Some(format!("{what} {}", s.name))
    }

    /// `a.b` / `C.this` / `O` for a singleton prefix; `None` for a prefix
    /// with no path to print.
    fn display_prefix(&self, pre: &Type) -> Option<String> {
        match pre {
            Type::ThisType(c) if !c.is_none() => Some(format!("{}.this", self.get(*c).name)),
            Type::ModuleRef(m) if !m.is_none() => {
                Some(self.get(*m).name.trim_end_matches('$').to_string())
            }
            Type::SingleType { prefix, sym } if !sym.is_none() => {
                let name = self.get(*sym).name.clone();
                match prefix.as_ref() {
                    Type::SingleType { .. } => match self.display_prefix(prefix) {
                        Some(p) => Some(format!("{p}.{name}")),
                        None => Some(name),
                    },
                    _ => Some(name),
                }
            }
            _ => None,
        }
    }

    pub fn display_type(&self, ty: &Type) -> String {
        // The as-seen-from view of `A#B` prints as `B`: its decls are the
        // compiler's bookkeeping, not something the program wrote. A stable
        // prefix it carries is printed the way nsc prints it -- `a.In`,
        // `Outer.this.In` -- because that is the whole difference between
        // the two types a diagnostic is telling apart.
        if let Some(p) = Self::as_seen_from_view(ty) {
            let core = self.display_type(p);
            return match crate::prefix::view_prefix(ty) {
                Some(pre) if self.is_singleton_prefix(pre) => match self.display_prefix(pre) {
                    Some(s) => format!("{s}.{core}"),
                    None => core,
                },
                _ => core,
            };
        }
        if let Some(s) = self.display_type_lambda(ty) {
            return s;
        }
        match ty {
            Type::Class { sym, args } => {
                let mut s = self.get(*sym).name.clone();
                if !args.is_empty() {
                    s.push('[');
                    s.push_str(
                        &args
                            .iter()
                            .map(|a| self.display_type(a))
                            .collect::<Vec<_>>()
                            .join(", "),
                    );
                    s.push(']');
                }
                s
            }
            Type::ModuleRef(id) => self.get(*id).name.clone(),
            Type::TypeParam(id) => {
                let name = self.get(*id).name.clone();
                if self.qualify_tparams.borrow().contains(id) {
                    match self.tparam_owner_desc(*id) {
                        Some(owner) => format!("{name} (defined in {owner})"),
                        None => name,
                    }
                } else {
                    name
                }
            }
            Type::Applied { ctor, args } => {
                let mut s = self.display_type(ctor);
                s.push('[');
                s.push_str(
                    &args
                        .iter()
                        .map(|a| self.display_type(a))
                        .collect::<Vec<_>>()
                        .join(", "),
                );
                s.push(']');
                s
            }
            Type::TypeMember(id) => {
                let s = self.get(*id);
                // An abstract projection prints the way the program wrote it.
                if self.abs_projection_of.contains_key(id) {
                    return format!("{}#{}", self.get(s.owner).name, s.name);
                }
                format!("{}.{}", self.get(s.owner).name, s.name)
            }
            Type::ThisType(id) => format!("{}.this.type", self.get(*id).name),
            Type::Constant(lit) => format!("{lit}"),
            Type::SingleType { sym, .. } => format!("{}.type", self.get(*sym).name),
            Type::Annotated { tpe, annot } => {
                format!("{} @{}", self.display_type(tpe), annot)
            }
            Type::BoundedWildcard { lo, hi } => {
                let mut s = String::from("_");
                if let Some(t) = lo {
                    s.push_str(" >: ");
                    s.push_str(&self.display_type(t));
                }
                if let Some(t) = hi {
                    s.push_str(" <: ");
                    s.push_str(&self.display_type(t));
                }
                s
            }
            Type::Refined { parents, decls } => {
                let mut s = String::new();
                if parents.is_empty() {
                    s.push_str("{ ");
                } else {
                    for (i, p) in parents.iter().enumerate() {
                        if i > 0 {
                            s.push_str(" with ");
                        }
                        s.push_str(&self.display_type(p));
                    }
                    if decls.is_empty() {
                        return s;
                    }
                    s.push_str(" { ");
                }
                for (i, d) in decls.iter().enumerate() {
                    if i > 0 {
                        s.push_str("; ");
                    }
                    s.push_str(&self.display_refine_decl(d));
                }
                s.push_str(" }");
                s
            }
            Type::Array(t) => format!("Array[{}]", self.display_type(t)),
            Type::Method { paramss, ret } => {
                let mut s = String::new();
                for ps in paramss {
                    s.push('(');
                    s.push_str(
                        &ps.iter()
                            .map(|p| self.display_type(p))
                            .collect::<Vec<_>>()
                            .join(", "),
                    );
                    s.push(')');
                }
                s.push_str(&self.display_type(ret));
                s
            }
            Type::Function { params, ret } => {
                let p = params
                    .iter()
                    .map(|p| self.display_type(p))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("({}) => {}", p, self.display_type(ret))
            }
            Type::Overload(alts) => format!(
                "<overload {}>",
                alts.iter()
                    .map(|a| self.display_type(a))
                    .collect::<Vec<_>>()
                    .join(" | ")
            ),
            Type::Repeated(t) => format!("{}*", self.display_type(t)),
            Type::ByName(t) => format!("=> {}", self.display_type(t)),
            // Without these the fallback `Display` runs, and it has no symbol
            // table: every class inside a tuple prints as `#4711`.
            Type::Tuple(ts) => format!(
                "({})",
                ts.iter()
                    .map(|t| self.display_type(t))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Type::Named { name, args } if !args.is_empty() => format!(
                "{name}[{}]",
                args.iter()
                    .map(|t| self.display_type(t))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            other => other.to_string(),
        }
    }

    pub fn jvm_internal(&self, id: SymbolId) -> String {
        let s = self.get(id);
        if !s.jvm_name.is_empty() {
            return s.jvm_name.clone();
        }
        // walk owners
        let mut parts = vec![s.name.clone()];
        let mut o = s.owner;
        while !o.is_none() && self.get(o).kind == SymKind::Package && self.get(o).name != "<_root_>"
        {
            parts.push(self.get(o).name.clone());
            o = self.get(o).owner;
        }
        parts.reverse();
        parts.join("/")
    }

    /// Is this refinement the *as-seen-from view* of a type projection
    /// (`A#B`) rather than a refinement the program wrote?
    ///
    /// See `Checker::projected_class_type`. Such a view constrains nothing --
    /// it only records what the prefix settles -- so subtyping, display and
    /// pickling read it as the bare parent.
    pub fn as_seen_from_view(ty: &Type) -> Option<&Type> {
        let Type::Refined { parents, decls } = ty else {
            return None;
        };
        if !decls
            .iter()
            .any(|d| matches!(d, RefineDecl::Type { name, .. } if name == AS_SEEN_FROM_MARK))
        {
            return None;
        }
        parents.first()
    }

    /// Every type member `cls` (or an ancestor of it) leaves abstract.
    ///
    /// Used by `A#B` projection: these are the names whose meaning the
    /// projection prefix can settle.
    /// For a type-projection view ([`PROJECTION_MARK`]): the projected class
    /// and the names its prefix settled.
    pub(crate) fn type_projection_view(ty: &Type) -> Option<(&Type, Vec<&str>)> {
        let parent = Self::as_seen_from_view(ty)?;
        let Type::Refined { decls, .. } = ty else {
            return None;
        };
        if !decls
            .iter()
            .any(|d| matches!(d, RefineDecl::Type { name, .. } if name == PROJECTION_MARK))
        {
            return None;
        }
        let settled = decls
            .iter()
            .filter_map(|d| match d {
                RefineDecl::Type { name, .. }
                    if name != AS_SEEN_FROM_MARK && name != PROJECTION_MARK =>
                {
                    Some(name.as_str())
                }
                _ => None,
            })
            .collect();
        Some((parent, settled))
    }

    pub(crate) fn abstract_type_member_names(&self, cls: SymbolId) -> Vec<String> {
        let mut out = Vec::new();
        let mut seen = rustc_hash::FxHashSet::default();
        let mut work = vec![cls];
        let mut visited = rustc_hash::FxHashSet::default();
        while let Some(id) = work.pop() {
            if !visited.insert(id.0) {
                continue;
            }
            for m in &self.get(id).members {
                let info = self.get(*m);
                if info.kind != SymKind::TypeMember {
                    continue;
                }
                let abstract_ = match &info.ty {
                    Type::NoType | Type::Error => true,
                    Type::TypeMember(inner) => inner == m,
                    _ => false,
                };
                if abstract_ && seen.insert(info.name.clone()) {
                    out.push(info.name.clone());
                }
            }
            for p in &self.get(id).parents {
                if let Some(ps) = self.class_sym_of(p) {
                    work.push(ps);
                }
            }
        }
        out
    }

    /// The type members `owner` has under `name`, with the ones that fix a
    /// right-hand side ahead of the ones left deferred.
    ///
    /// `lookup_member` walks the parents depth-first, so an abstract `type
    /// Session` declared in `slick.basic.BasicBackend` can come out ahead of
    /// the `type Session = SessionDef` that `slick.jdbc.JdbcBackend` fixes it
    /// to. nsc resolves the same name in linearisation order, where a
    /// concrete definition overrides a deferred one; reading the deferred one
    /// leaves an opaque type with no members at all.
    pub(crate) fn type_members_named(&self, owner: SymbolId, name: &str) -> Vec<SymbolId> {
        let mut found = self.lookup_member(owner, name);
        found.sort_by_key(|&m| u8::from(self.is_deferred_type_member(m)));
        found
    }

    /// `from` and the class-like symbols lexically enclosing it, innermost first.
    pub(crate) fn enclosing_classes(&self, from: SymbolId) -> Vec<SymbolId> {
        let mut out = Vec::new();
        let mut cur = from;
        while !cur.is_none() {
            if self.get(cur).is_class_like() || out.is_empty() {
                out.push(cur);
            }
            if out.len() > 16 {
                break;
            }
            let owner = self.get(cur).owner;
            if owner == cur {
                break;
            }
            cur = owner;
        }
        out
    }

    /// `self.T` -- a path member whose path is a class's *self alias* -- read
    /// from a class that supplies the instance `self` names.
    ///
    /// A self alias is another spelling of `this`, so a `self.T` written
    /// inside `O` denotes `O.this.T`; seen from a class that inherits `O`, the
    /// member `O` left deferred may well be fixed. slick's profile cake is
    /// that shape, and `tmember1.scala` is it cut down:
    ///
    /// ```scala
    /// trait Profile extends TypesComponent { self: Profile =>
    ///   trait API { type ColumnType[T] = self.ColumnType[T] }
    /// }
    /// object Main extends JdbcProfile { object api extends API }   // type ColumnType[T] = JdbcType[T]
    /// ```
    ///
    /// `api.ColumnType[Int]` inside `Main` is `JdbcType[Int]`, because `Main`
    /// is the `Profile` that `API`'s `self` names. Before `agent/hkpath` the
    /// alias recorded the bare declaration and this fell out of the ordinary
    /// name walk below; with a prefix on it, it has to be asked for here.
    ///
    /// Two guards keep it from becoming an unconditional widening:
    ///
    /// * a class that leaves `T` deferred answers `None`. `p.T` and `q.T` on
    ///   two such prefixes must stay distinct, and handing back the bare
    ///   declaration is exactly the widening `agent/projection` removed.
    /// * a class whose own `T` is an alias that *names this very path member*
    ///   answers `None` as well. cats' `Representable#compose` writes `type Rp
    ///   = (self.Rp, other.Rp)` in an anonymous `Representable` subclass, where
    ///   `self` is the *outer* `Representable`; resolving it by name in the
    ///   subclass is the right-hand side folding back onto itself, and it is
    ///   the reason a path member is refused the name walk in the first place.
    fn self_alias_member_at(&self, from: SymbolId, id: SymbolId) -> Option<Type> {
        if WRITTEN_TYPE.with(|c| c.get()) {
            return None;
        }
        let decl = *self.path_member_decl.get(&id)?;
        let &[head] = self.path_member_path.get(&id)?.as_slice() else {
            return None;
        };
        let owner = self.get(head).owner;
        if owner.is_none() || self.get(owner).self_alias != Some(head) {
            return None;
        }
        let name = self.get(decl).name.clone();
        for cls in self.enclosing_classes(from) {
            // Only a class that really is one of these supplies the instance:
            // an enclosing class that never inherited `owner` is some other
            // object, and its member of the same name is a different member.
            if cls != owner && !self.is_ancestor_of(owner, cls) {
                continue;
            }
            let m = self
                .type_members_named(cls, &name)
                .into_iter()
                .find(|m| self.get(*m).kind == SymKind::TypeMember)?;
            if self.is_deferred_type_member(m) {
                return None;
            }
            if self.path_members_in(&self.get(m).ty).contains(&id) {
                return None;
            }
            if !self.get(m).tparams.is_empty() {
                return Some(Type::TypeMember(m));
            }
            let t = self.get(m).ty.clone();
            let _guard = enter_alias(m)?;
            return Some(self.expand_type_members(cls, &t));
        }
        None
    }

    /// [`Self::expand_type_members`] for re-reading a type the *source wrote*
    /// from the class being typed, where `from` is `this_class` rather than
    /// anything the type itself names.
    ///
    /// The difference is `self.T`. A written type may carry its own prefix,
    /// and then the reading class is not what settles it:
    /// `Mem.api.ColumnType[Int]` inside `object Jdbc` is `Mem`'s member and
    /// stays `MemType[Int]`. The prefix-driven reduction happens where the
    /// prefix is still in hand (`Typer::with_prefix_if_type_member`); here it
    /// is suppressed.
    pub fn expand_written_type(&self, from: SymbolId, ty: &Type) -> Type {
        let saved = WRITTEN_TYPE.with(|c| c.replace(true));
        let _guard = WrittenGuard(saved);
        self.expand_type_members(from, ty)
    }

    /// Replace abstract type members with aliases defined on `from` (and parents).
    pub fn expand_type_members(&self, from: SymbolId, ty: &Type) -> Type {
        match ty {
            Type::TypeMember(id) => {
                // Refinement placeholders are allocated with no owner. Do not
                // replace them with the parent's abstract member of the same name
                // (`{ type A <: Int }` must keep `A`'s bound).
                if self.get(*id).owner.is_none() {
                    return ty.clone();
                }
                // A path-dependent member already says which prefix it came
                // through. Re-resolving its *name* in `from` is what dropping
                // the prefix looked like: `q.T` would come back as `A`'s own
                // `T`, and `p.T` with it. An abstract projection is the same
                // case one step further out: `API`'s `type Session =
                // Backend#Session` must not come back as `API`'s own
                // `Session`, which is the alias being expanded.
                if self.path_member_decl.contains_key(id) || self.abs_projection_of.contains_key(id)
                {
                    return self
                        .self_alias_member_at(from, *id)
                        .unwrap_or_else(|| ty.clone());
                }
                let name = self.get(*id).name.clone();
                // `from` first, then its lexically enclosing classes: an inner
                // class (`Main.factory: Main.Factory`) sees `Main`'s implementation
                // of an abstract member declared beside `Factory`, which is what
                // nsc reaches through the outer-instance prefix.
                for owner in self.enclosing_classes(from) {
                    for m in self.type_members_named(owner, &name) {
                        if self.get(m).kind == SymKind::TypeMember {
                            if !self.get(m).tparams.is_empty() {
                                return Type::TypeMember(m);
                            }
                            let t = self.get(m).ty.clone();
                            if matches!(t, Type::TypeMember(_) | Type::NoType | Type::Error) {
                                return Type::TypeMember(m);
                            }
                            let Some(_guard) = enter_alias(m) else {
                                return Type::TypeMember(m);
                            };
                            return self.expand_type_members(from, &t);
                        }
                    }
                }
                ty.clone()
            }
            Type::Class { sym, args } => Type::Class {
                sym: *sym,
                args: args
                    .iter()
                    .map(|a| self.expand_type_members(from, a))
                    .collect(),
            },
            Type::Applied { ctor, args } => {
                let applied = apply_type_ctor(
                    self.expand_type_members(from, ctor),
                    args.iter()
                        .map(|a| self.expand_type_members(from, a))
                        .collect(),
                );
                self.expand_applied_hk_alias(applied)
            }
            Type::Array(t) => Type::Array(Box::new(self.expand_type_members(from, t))),
            Type::Function { params, ret } => Type::Function {
                params: params
                    .iter()
                    .map(|p| self.expand_type_members(from, p))
                    .collect(),
                ret: Box::new(self.expand_type_members(from, ret)),
            },
            Type::Method { paramss, ret } => Type::Method {
                paramss: paramss
                    .iter()
                    .map(|ps| {
                        ps.iter()
                            .map(|p| self.expand_type_members(from, p))
                            .collect()
                    })
                    .collect(),
                ret: Box::new(self.expand_type_members(from, ret)),
            },
            Type::ByName(t) => Type::ByName(Box::new(self.expand_type_members(from, t))),
            Type::Repeated(t) => Type::Repeated(Box::new(self.expand_type_members(from, t))),
            Type::Tuple(ts) => Type::Tuple(
                ts.iter()
                    .map(|t| self.expand_type_members(from, t))
                    .collect(),
            ),
            Type::Refined { parents, decls } => Type::Refined {
                parents: parents
                    .iter()
                    .map(|p| self.expand_type_members(from, p))
                    .collect(),
                decls: decls
                    .iter()
                    .map(|d| expand_refine_decl(self, from, d))
                    .collect(),
            },
            Type::Annotated { tpe, annot } => Type::Annotated {
                tpe: Box::new(self.expand_type_members(from, tpe)),
                annot: annot.clone(),
            },
            Type::BoundedWildcard { lo, hi } => Type::BoundedWildcard {
                lo: lo
                    .as_ref()
                    .map(|t| Box::new(self.expand_type_members(from, t))),
                hi: hi
                    .as_ref()
                    .map(|t| Box::new(self.expand_type_members(from, t))),
            },
            Type::SingleType { prefix, sym } => Type::SingleType {
                prefix: Box::new(self.expand_type_members(from, prefix)),
                sym: *sym,
            },
            other => other.clone(),
        }
    }

    /// Expand type members using aliases on a (possibly refined) prefix type.
    pub fn expand_in_type(&self, from: &Type, ty: &Type) -> Type {
        match from {
            Type::Refined { parents, decls } => {
                let mut t = subst_refine_aliases(self, decls, ty);
                for p in parents {
                    t = self.expand_in_type(p, &t);
                }
                t
            }
            Type::Class { sym, args } => {
                let t = self.expand_type_members(*sym, ty);
                // The alias's right-hand side is written in its owner's
                // vocabulary: `type Self = Base[T]` reached through a
                // `Base[String]` is a `Base[String]`, not a `Base[T]`.
                if args.is_empty() || t == *ty {
                    t
                } else {
                    self.subst_as_seen_from(from, &t)
                }
            }
            Type::ModuleRef(sym) => self.expand_type_members(*sym, ty),
            Type::ThisType(sym) => self.expand_type_members(*sym, ty),
            Type::Annotated { tpe, .. } => self.expand_in_type(tpe, ty),
            Type::SingleType { prefix, sym } => {
                let t = self.singleton_underlying(*sym);
                if t.is_no_type() {
                    self.expand_in_type(prefix, ty)
                } else {
                    self.expand_in_type(&t, ty)
                }
            }
            _ => {
                if let Some(c) = self.class_sym_of(from) {
                    self.expand_type_members(c, ty)
                } else {
                    ty.clone()
                }
            }
        }
    }

    pub fn refine_member_type(decls: &[RefineDecl], name: &str) -> Option<Type> {
        for d in decls {
            match d {
                RefineDecl::Def {
                    name: n,
                    paramss,
                    ret,
                } if n == name => {
                    return Some(Type::Method {
                        paramss: paramss.clone(),
                        ret: Box::new(ret.clone()),
                    });
                }
                RefineDecl::Val { name: n, ty } if n == name => return Some(ty.clone()),
                RefineDecl::Type { name: n, rhs, .. } if n == name => {
                    return Some(rhs.clone().unwrap_or(Type::Named {
                        name: n.clone(),
                        args: vec![],
                    }));
                }
                _ => {}
            }
        }
        None
    }

    pub fn refined_has_term_members(decls: &[RefineDecl]) -> bool {
        decls
            .iter()
            .any(|d| matches!(d, RefineDecl::Def { .. } | RefineDecl::Val { .. }))
    }

    fn conforms_to_refinement(&self, a: &Type, decls: &[RefineDecl]) -> bool {
        for d in decls {
            match d {
                RefineDecl::Type {
                    name,
                    rhs,
                    tparams,
                    hi,
                    ..
                } => {
                    let Some(have) = self.lookup_type_member_on(a, name) else {
                        return false;
                    };
                    if *tparams > 0 && self.kind_arity(&have) != *tparams {
                        return false;
                    }
                    if let Some(want) = rhs {
                        // Abstract `{ type A <: T }` / `{ type F[_] }` store a
                        // TypeMember placeholder; only aliases constrain equality.
                        let abstract_placeholder = match want {
                            Type::TypeMember(id) => matches!(
                                &self.get(*id).ty,
                                Type::TypeMember(_) | Type::NoType | Type::Error
                            ),
                            _ => false,
                        };
                        if !abstract_placeholder {
                            if *tparams > 0 {
                                let args: Vec<Type> = (0..*tparams).map(|_| Type::Int).collect();
                                let have_app = self.expand_applied_hk_alias(apply_type_ctor(
                                    have.clone(),
                                    args.clone(),
                                ));
                                let want_app = self
                                    .expand_applied_hk_alias(apply_type_ctor(want.clone(), args));
                                if !self.types_same_enough(&have_app, &want_app) {
                                    return false;
                                }
                            } else if !self.types_same_enough(&have, want) {
                                return false;
                            }
                        }
                    }
                    if let Some(h) = hi {
                        if *tparams > 0 {
                            let args: Vec<Type> = (0..*tparams).map(|_| Type::Int).collect();
                            let have_app =
                                self.expand_applied_hk_alias(apply_type_ctor(have.clone(), args));
                            if !self.is_sub_type(&have_app, h) {
                                return false;
                            }
                        } else if !self.is_sub_type(&have, h) {
                            return false;
                        }
                    }
                }
                RefineDecl::Def { name, ret, .. } => {
                    let Some(have) = self.lookup_term_member_on(a, name) else {
                        return false;
                    };
                    if !self.is_sub_type(have.result(), ret) {
                        return false;
                    }
                }
                RefineDecl::Val { name, ty } => {
                    let Some(have) = self.lookup_term_member_on(a, name) else {
                        return false;
                    };
                    if !self.is_sub_type(have.result(), ty) {
                        return false;
                    }
                }
            }
        }
        true
    }

    fn types_same_enough(&self, a: &Type, b: &Type) -> bool {
        a == b || (self.is_sub_type(a, b) && self.is_sub_type(b, a))
    }

    pub(crate) fn lookup_type_member_on(&self, ty: &Type, name: &str) -> Option<Type> {
        if let Type::Refined { parents, decls } = ty {
            if let Some(t) = Self::refine_member_type(decls, name) {
                if decls
                    .iter()
                    .any(|d| matches!(d, RefineDecl::Type { name: n, .. } if n == name))
                {
                    return Some(t);
                }
            }
            for p in parents {
                if let Some(t) = self.lookup_type_member_on(p, name) {
                    return Some(t);
                }
            }
        }
        let cls = self.class_sym_of(ty)?;
        let found = self.lookup_member(cls, name);
        for m in found {
            if self.get(m).kind == SymKind::TypeMember {
                if !self.get(m).tparams.is_empty() {
                    return Some(self.expand_in_type(ty, &Type::TypeMember(m)));
                }
                let rhs = self.get(m).ty.clone();
                return Some(match rhs {
                    Type::NoType | Type::Error | Type::TypeMember(_) => {
                        self.expand_in_type(ty, &Type::TypeMember(m))
                    }
                    other => self.expand_in_type(ty, &other),
                });
            }
        }
        None
    }

    fn lookup_term_member_on(&self, ty: &Type, name: &str) -> Option<Type> {
        if let Type::Refined { parents, decls } = ty {
            if decls.iter().any(|d| {
                matches!(
                    d,
                    RefineDecl::Def { name: n, .. } | RefineDecl::Val { name: n, .. } if n == name
                )
            }) {
                return Self::refine_member_type(decls, name);
            }
            for p in parents {
                if let Some(t) = self.lookup_term_member_on(p, name) {
                    return Some(t);
                }
            }
        }
        let cls = self.class_sym_of(ty)?;
        self.lookup_member(cls, name).into_iter().find_map(|m| {
            let s = self.get(m);
            match s.kind {
                SymKind::Method | SymKind::Term => Some(self.expand_in_type(ty, &s.ty)),
                _ => None,
            }
        })
    }

    /// SIP-21: exactly one abstract method (not an Object method / FunctionN).
    pub fn sam_sig(&self, ty: &Type) -> Option<SamSig> {
        self.sam_sig_over(ty, &[])
    }

    /// [`Self::sam_sig`], told which inherited declarations the class in fact
    /// **overrides** without the symbol table having been shown the override.
    ///
    /// `PickleSupply` installs a library class's members one name at a time,
    /// on demand, so an override nothing has asked for is indistinguishable
    /// from an override that is not there. `scala.math.Ordering` is the shape
    /// that matters: it inherits a deferred `equiv` from `Equiv` (through
    /// `PartialOrdering`) and overrides it -- `javap -p scala.math.Ordering`
    /// says `public default boolean equiv(T, T)` -- so until something names
    /// `equiv` on an `Ordering`, the class reads as having two abstract
    /// methods and no function literal converts to it.
    ///
    /// The caller reads the missing half straight out of the pickle
    /// (`PickleSupply::concrete_method_names`) and passes it here rather than
    /// installing it; see that function for what installing it costs.
    pub fn sam_sig_over(&self, ty: &Type, overridden: &[String]) -> Option<SamSig> {
        // An inner class behind a prefix (`prefix.rs`) is the class it views;
        // the prefix is what its members' bare inner classes are read at.
        let view_pre = crate::prefix::view_prefix(ty).cloned();
        let ty = crate::prefix::strip_view(ty);
        let cls = self.class_sym_of(ty)?;
        let jvm = self.get(cls).jvm_name.clone();
        if jvm.starts_with("scala/Function") || jvm.ends_with("PartialFunction") {
            return None;
        }
        // nsc `samOf`: a SAM *class* must be instantiable by the literal's
        // anonymous subclass with no arguments -- its constructor takes an
        // empty parameter list. `abstract class H(x: Int) { def h(s: String):
        // String }` is not a SAM type; converting to it built a subclass
        // calling a `<init>()V` that does not exist.
        let f = self.get(cls).flags;
        if !f.contains(Flags::TRAIT) && !f.contains(Flags::INTERFACE) {
            let ctors: Vec<SymbolId> = self
                .get(cls)
                .members
                .iter()
                .copied()
                .filter(|m| self.get(*m).name == "<init>")
                .collect();
            let nullary = |c: &SymbolId| match &self.get(*c).ty {
                Type::Method { paramss, .. } => paramss.iter().all(|l| l.is_empty()),
                _ => self.get(*c).params.is_empty(),
            };
            if !ctors.is_empty() && !ctors.iter().any(nullary) {
                return None;
            }
        }
        let mut abstracts = self.abstract_sam_methods(cls);
        let known = self.sam_known_overrides.get(&cls.0);
        if !overridden.is_empty() || known.is_some() {
            abstracts.retain(|m| {
                let s = self.get(*m);
                s.owner == cls
                    || !(overridden.contains(&s.name)
                        || known.is_some_and(|ns| ns.contains(&s.name)))
            });
        }
        if abstracts.len() != 1 {
            return None;
        }
        let method = abstracts[0];
        // nsc `definitions.samOf`: `!sam.isOverloaded`. `abstract_sam_methods`
        // keys by *name*, so that an override does not count twice as the
        // declaration it replaces -- which also collapses a genuinely
        // **overloaded** abstract method to one. `java.lang.Appendable`
        // declares three abstract `append`s and is no SAM; read as one, an
        // untyped function literal stayed a candidate for an `Appendable`
        // formal and `processFully(log err _)` was an `ambiguous overload`
        // against `processFully(processLine: String => Unit)`
        // (`sys/process/BasicIO.scala:160,161`).
        {
            let name = self.get(method).name.clone();
            let owner = self.get(method).owner;
            let same_name = self
                .get(owner)
                .members
                .iter()
                .copied()
                .filter(|&m| {
                    self.get(m).kind == SymKind::Method
                        && self.get(m).name == name
                        && self.method_is_deferred(m)
                })
                .count();
            if same_name > 1 {
                return None;
            }
        }
        // nsc `definitions.samOf`: `sam.typeParams.isEmpty`.
        // A polymorphic abstract method has no function type to convert from,
        // and real scalac 2.13.16 says so -- `trait Poly { def f[A](a: A): A }`
        // with `val p: Poly = x => x` is two errors, `missing parameter type`
        // and `found: ? => ?  required: Poly`. Without this the literal was
        // accepted and its parameter silently pinned to whatever the body
        // wanted.
        if !self.get(method).tparams.is_empty() {
            return None;
        }
        // The abstract method may be declared in a *parent* (`trait C[-T]
        // extends (T => R)` gets its `apply` from `Function1`), so its type has
        // to be read as seen from `ty` -- substituting only `cls`'s own type
        // parameters leaves the parent's untouched.
        // The function literal is typed against a ground SAM target. A Java
        // Consumer[_ >: String] accepts a String parameter, not an existential
        // value whose upper bound is Object. Keep the original expected type
        // on the adapted tree; only this method signature uses the bounds.
        let recv = match ty {
            Type::Class { sym, args } => Type::Class {
                sym: *sym,
                args: args
                    .iter()
                    .map(|arg| match arg {
                        Type::BoundedWildcard { lo, hi } => lo
                            .as_deref()
                            .filter(|t| !matches!(t, Type::Nothing))
                            .or(hi.as_deref())
                            .cloned()
                            .unwrap_or(Type::Any),
                        Type::Wildcard => Type::Any,
                        other => other.clone(),
                    })
                    .collect(),
            },
            _ => Type::Class {
                sym: cls,
                args: Vec::new(),
            },
        };
        let subst = |t: &Type| self.subst_as_seen_from_at(&recv, view_pre.as_ref(), t);
        let (raw_params, raw_ret) = match &self.get(method).ty {
            Type::Method { paramss, ret } => (
                paramss.iter().flatten().cloned().collect::<Vec<_>>(),
                (**ret).clone(),
            ),
            _ => return None,
        };
        Some(SamSig {
            class: cls,
            method,
            name: self.get(method).name.clone(),
            param_tys: raw_params.iter().map(subst).collect(),
            ret_ty: subst(&raw_ret),
            raw_param_tys: raw_params,
            raw_ret_ty: raw_ret,
        })
    }

    fn abstract_sam_methods(&self, cls: SymbolId) -> Vec<SymbolId> {
        let mut by_name: HashMap<String, SymbolId> = HashMap::default();
        let mut work = vec![cls];
        let mut seen = rustc_hash::FxHashSet::default();
        while let Some(id) = work.pop() {
            if !seen.insert(id.0) {
                continue;
            }
            for m in &self.get(id).members {
                let s = self.get(*m);
                if s.kind != SymKind::Method || sam_excluded_name(&s.name) {
                    continue;
                }
                by_name.entry(s.name.clone()).or_insert(*m);
            }
            for p in &self.get(id).parents {
                // A parent written as a function type (`trait C[-T] extends
                // (T => R)`) declares `apply`, which is what makes `C` a SAM.
                let as_class = self.function_class_form(p);
                let p = as_class.as_ref().unwrap_or(p);
                if let Some(c) = self.class_sym_of(p) {
                    work.push(c);
                }
            }
        }
        by_name
            .into_values()
            .filter(|m| self.method_is_deferred(*m))
            .collect()
    }

    /// Names of deferred methods `cls` inherits from a parent and does not
    /// itself declare a member for.
    ///
    /// `PickleSupply` installs a library class's members one name at a time,
    /// on demand, so a library trait's symbol carries only what some earlier
    /// expression asked for -- and an override it has not been asked for is
    /// indistinguishable from an override that is not there. That is fatal
    /// exactly for [`Self::sam_sig`], which has to count abstract methods:
    /// `scala.math.Ordering` inherits a deferred `equiv` from `Equiv` through
    /// `PartialOrdering` and overrides it concretely (`javap -p
    /// scala.math.Ordering` says `public default boolean equiv(T, T)`), so
    /// until that override is installed `Ordering` reads as having *two*
    /// abstract methods and no function literal converts to it.
    ///
    /// The caller completes these names and asks again. The list is bounded
    /// by what the parents declare, so nothing else is pulled in.
    pub fn inherited_deferred_method_names(&self, cls: SymbolId) -> Vec<String> {
        let own: rustc_hash::FxHashSet<&str> = self
            .get(cls)
            .members
            .iter()
            .map(|m| self.get(*m).name.as_str())
            .collect();
        self.abstract_sam_methods(cls)
            .into_iter()
            .filter(|m| self.get(*m).owner != cls)
            .map(|m| self.get(m).name.clone())
            .filter(|n| !own.contains(n.as_str()))
            .collect()
    }

    /// How many abstract methods `cls` reads as having, SAM rules applied.
    pub fn sam_method_count(&self, cls: SymbolId) -> usize {
        self.abstract_sam_methods(cls).len()
    }

    /// Whether a method has no implementation where it is declared.
    ///
    /// `Flags::ABSTRACT` alone answers this only for the two supplies that can
    /// set it: this run's own sources, and the eager `-cp` classfile scan.
    /// The prelude and `PickleSupply` record the same fact in
    /// [`Symbol::deferred_method`] instead, so every SAM type that comes out
    /// of the scala-library jar (`scala.math.Equiv`, `scala.math.Ordering`,
    /// `scala.util.hashing.Hashing`, ...) was read as having *zero* abstract
    /// methods and so was not a SAM type at all.
    pub fn method_is_deferred(&self, m: SymbolId) -> bool {
        let s = self.get(m);
        s.flags.contains(Flags::ABSTRACT) || s.deferred_method
    }
}

/// SAM conversion target (class + single abstract method).
#[derive(Clone, Debug)]
pub struct SamSig {
    pub class: SymbolId,
    pub method: SymbolId,
    pub name: String,
    pub param_tys: Vec<Type>,
    pub ret_ty: Type,
    pub raw_param_tys: Vec<Type>,
    pub raw_ret_ty: Type,
}

fn sam_excluded_name(name: &str) -> bool {
    matches!(
        name,
        "<init>"
            | "<clinit>"
            | "$init$"
            | "equals"
            | "hashCode"
            | "toString"
            | "clone"
            | "finalize"
            | "wait"
            | "notify"
            | "notifyAll"
            | "getClass"
            | "asInstanceOf"
            | "isInstanceOf"
            | "=="
            | "!="
            | "eq"
            | "ne"
            | "##"
            | "synchronized"
    )
}

impl Default for SymbolTable {
    fn default() -> Self {
        Self::new()
    }
}

fn subst_map(ty: &Type, tps: &[scala_rs_parser::SymbolId], args: &[Type]) -> Type {
    match ty {
        Type::TypeParam(id) => tps
            .iter()
            .position(|t| t == id)
            .and_then(|i| args.get(i).cloned())
            .unwrap_or_else(|| ty.clone()),
        Type::TypeMember(_) => ty.clone(),
        Type::Class { sym, args: as_ } => Type::Class {
            sym: *sym,
            args: as_.iter().map(|a| subst_map(a, tps, args)).collect(),
        },
        Type::Applied { ctor, args: as_ } => apply_type_ctor(
            subst_map(ctor, tps, args),
            as_.iter().map(|a| subst_map(a, tps, args)).collect(),
        ),
        Type::Array(t) => Type::Array(Box::new(subst_map(t, tps, args))),
        Type::Function { params, ret } => Type::Function {
            params: params.iter().map(|p| subst_map(p, tps, args)).collect(),
            ret: Box::new(subst_map(ret, tps, args)),
        },
        Type::Method { paramss, ret } => Type::Method {
            paramss: paramss
                .iter()
                .map(|ps| ps.iter().map(|p| subst_map(p, tps, args)).collect())
                .collect(),
            ret: Box::new(subst_map(ret, tps, args)),
        },
        Type::ByName(t) => Type::ByName(Box::new(subst_map(t, tps, args))),
        Type::Repeated(t) => Type::Repeated(Box::new(subst_map(t, tps, args))),
        Type::Tuple(ts) => Type::Tuple(ts.iter().map(|t| subst_map(t, tps, args)).collect()),
        Type::Named { name, args: as_ } => Type::Named {
            name: name.clone(),
            args: as_.iter().map(|a| subst_map(a, tps, args)).collect(),
        },
        Type::Refined { parents, decls } => Type::Refined {
            parents: parents.iter().map(|p| subst_map(p, tps, args)).collect(),
            decls: decls
                .iter()
                .map(|d| subst_refine_decl(d, tps, args))
                .collect(),
        },
        Type::Annotated { tpe, annot } => Type::Annotated {
            tpe: Box::new(subst_map(tpe, tps, args)),
            annot: annot.clone(),
        },
        Type::BoundedWildcard { lo, hi } => Type::BoundedWildcard {
            lo: lo.as_ref().map(|t| Box::new(subst_map(t, tps, args))),
            hi: hi.as_ref().map(|t| Box::new(subst_map(t, tps, args))),
        },
        Type::SingleType { prefix, sym } => Type::SingleType {
            prefix: Box::new(subst_map(prefix, tps, args)),
            sym: *sym,
        },
        other => other.clone(),
    }
}

fn expand_refine_decl(st: &SymbolTable, from: SymbolId, d: &RefineDecl) -> RefineDecl {
    match d {
        RefineDecl::Type {
            name,
            rhs,
            tparams,
            lo,
            hi,
        } => RefineDecl::Type {
            name: name.clone(),
            rhs: rhs.as_ref().map(|t| st.expand_type_members(from, t)),
            tparams: *tparams,
            lo: lo.as_ref().map(|t| st.expand_type_members(from, t)),
            hi: hi.as_ref().map(|t| st.expand_type_members(from, t)),
        },
        RefineDecl::Def { name, paramss, ret } => RefineDecl::Def {
            name: name.clone(),
            paramss: paramss
                .iter()
                .map(|ps| ps.iter().map(|p| st.expand_type_members(from, p)).collect())
                .collect(),
            ret: st.expand_type_members(from, ret),
        },
        RefineDecl::Val { name, ty } => RefineDecl::Val {
            name: name.clone(),
            ty: st.expand_type_members(from, ty),
        },
    }
}

fn subst_refine_decl(
    d: &RefineDecl,
    tps: &[scala_rs_parser::SymbolId],
    args: &[Type],
) -> RefineDecl {
    match d {
        RefineDecl::Type {
            name,
            rhs,
            tparams,
            lo,
            hi,
        } => RefineDecl::Type {
            name: name.clone(),
            rhs: rhs.as_ref().map(|t| subst_map(t, tps, args)),
            tparams: *tparams,
            lo: lo.as_ref().map(|t| subst_map(t, tps, args)),
            hi: hi.as_ref().map(|t| subst_map(t, tps, args)),
        },
        RefineDecl::Def { name, paramss, ret } => RefineDecl::Def {
            name: name.clone(),
            paramss: paramss
                .iter()
                .map(|ps| ps.iter().map(|p| subst_map(p, tps, args)).collect())
                .collect(),
            ret: subst_map(ret, tps, args),
        },
        RefineDecl::Val { name, ty } => RefineDecl::Val {
            name: name.clone(),
            ty: subst_map(ty, tps, args),
        },
    }
}

fn expand_hk_refine_decl(st: &SymbolTable, d: &RefineDecl) -> RefineDecl {
    match d {
        // The `rhs` of a higher-kinded member is the folded lambda itself
        // (possibly partially applied to what it captured); reducing it here
        // would throw its parameters away.
        RefineDecl::Type { .. } => d.clone(),
        RefineDecl::Def { name, paramss, ret } => RefineDecl::Def {
            name: name.clone(),
            paramss: paramss
                .iter()
                .map(|ps| ps.iter().map(|p| st.expand_hk_aliases(p)).collect())
                .collect(),
            ret: st.expand_hk_aliases(ret),
        },
        RefineDecl::Val { name, ty } => RefineDecl::Val {
            name: name.clone(),
            ty: st.expand_hk_aliases(ty),
        },
    }
}

fn subst_refine_aliases(st: &SymbolTable, decls: &[RefineDecl], ty: &Type) -> Type {
    subst_refine_aliases_seen(st, decls, ty, &mut Vec::new())
}

/// `subst_refine_aliases`, carrying the members whose right-hand side is
/// already being substituted into.
///
/// A refinement's alias can name itself. cats' `Representable#compose` builds
/// an anonymous class declaring
/// `type Representation = (self.Representation, G.Representation)` over a
/// parent that declares `Representation` abstract, and a `TypeMember` here has
/// no prefix to tell `self.` from `G.`, so both collapse onto the member being
/// defined and the right-hand side reads `(Representation, Representation)`.
/// Expanding it again is what the `Tuple` arm below does, and the recursion
/// only ends with the process: 512 MB of stack, no diagnostic, and a cats
/// measurement of `errors=0 classes=0`. `expand_type_members` already stops at
/// the second visit for the same shape; this does the same, leaving the member
/// unexpanded rather than inventing an answer.
fn subst_refine_aliases_seen(
    st: &SymbolTable,
    decls: &[RefineDecl],
    ty: &Type,
    seen: &mut Vec<String>,
) -> Type {
    match ty {
        Type::TypeMember(id) => {
            let name = st.get(*id).name.clone();
            for d in decls {
                if let RefineDecl::Type {
                    name: n,
                    rhs: Some(rhs),
                    ..
                } = d
                {
                    if n == &name {
                        // Placeholder `TypeMember` rhs is the refinement's own
                        // member (`{ type F[X] = Id[X] }` stores `TypeMember(id)`).
                        // Recursing would match the same decl forever. A lambda
                        // that captured an enclosing parameter stores that
                        // placeholder already applied to what it captured
                        // (`{ type F[x] = F0[x] }` in cats' `Parallel.Aux`), and
                        // that form is the same self-reference.
                        return match rhs {
                            Type::TypeMember(_) => rhs.clone(),
                            Type::Applied { ctor, .. }
                                if matches!(ctor.as_ref(), Type::TypeMember(_)) =>
                            {
                                rhs.clone()
                            }
                            // The same self-reference, but buried: the member
                            // stands somewhere inside its own right-hand side.
                            _ if seen.iter().any(|s| s == n) => ty.clone(),
                            _ => {
                                seen.push(name.clone());
                                let out = subst_refine_aliases_seen(st, decls, rhs, seen);
                                seen.pop();
                                out
                            }
                        };
                    }
                }
            }
            ty.clone()
        }
        Type::Class { sym, args } => Type::Class {
            sym: *sym,
            args: args
                .iter()
                .map(|a| subst_refine_aliases_seen(st, decls, a, seen))
                .collect(),
        },
        Type::Applied { ctor, args } => {
            let ctor = subst_refine_aliases_seen(st, decls, ctor, seen);
            let args: Vec<Type> = args
                .iter()
                .map(|a| subst_refine_aliases_seen(st, decls, a, seen))
                .collect();
            st.expand_applied_hk_alias(apply_type_ctor(ctor, args))
        }
        Type::Array(t) => Type::Array(Box::new(subst_refine_aliases_seen(st, decls, t, seen))),
        Type::Function { params, ret } => {
            let params = params
                .iter()
                .map(|p| subst_refine_aliases_seen(st, decls, p, seen))
                .collect();
            Type::Function {
                params,
                ret: Box::new(subst_refine_aliases_seen(st, decls, ret, seen)),
            }
        }
        Type::Method { paramss, ret } => {
            let paramss = paramss
                .iter()
                .map(|ps| {
                    ps.iter()
                        .map(|p| subst_refine_aliases_seen(st, decls, p, seen))
                        .collect()
                })
                .collect();
            Type::Method {
                paramss,
                ret: Box::new(subst_refine_aliases_seen(st, decls, ret, seen)),
            }
        }
        Type::ByName(t) => Type::ByName(Box::new(subst_refine_aliases_seen(st, decls, t, seen))),
        Type::Repeated(t) => {
            Type::Repeated(Box::new(subst_refine_aliases_seen(st, decls, t, seen)))
        }
        Type::Tuple(ts) => Type::Tuple(
            ts.iter()
                .map(|t| subst_refine_aliases_seen(st, decls, t, seen))
                .collect(),
        ),
        other => other.clone(),
    }
}

pub(crate) fn subst_tparams_slice(tps: &[SymbolId], args: &[Type], ty: &Type) -> Type {
    subst_map(ty, tps, args)
}

/// `subst_tparams_slice` without the copy when the substitution is the
/// identity.
///
/// With no parameters to replace, or no arguments to replace them by,
/// `subst_map` rebuilds the whole type only to hand back what it was given:
/// `position()` finds nothing in an empty `tps`, and `args.get(i)` is `None`
/// for every `i` when `args` is empty, so both arms fall through to
/// `ty.clone()`. Most classes in a hierarchy walk are not generic, so this is
/// the common case rather than a corner.
///
/// This is *not* the fast path an earlier pass measured and discarded (test
/// whether `ty` mentions any of `tps`): that one walks the type and the types
/// on this path really do mention their parameters. This one looks only at the
/// two slice lengths.
pub(crate) fn subst_tparams_cow<'a>(
    tps: &[SymbolId],
    args: &[Type],
    ty: &'a Type,
) -> std::borrow::Cow<'a, Type> {
    if tps.is_empty() || args.is_empty() {
        std::borrow::Cow::Borrowed(ty)
    } else {
        std::borrow::Cow::Owned(subst_map(ty, tps, args))
    }
}

/// Does any node of `ty` satisfy `f`? Walks the same shapes as `map_type`
/// without rebuilding anything.
pub(crate) fn any_type(ty: &Type, f: &mut impl FnMut(&Type) -> bool) -> bool {
    if f(ty) {
        return true;
    }
    let some = |ts: &[Type], f: &mut _| ts.iter().any(|t| any_type(t, f));
    match ty {
        Type::Class { args, .. }
        | Type::Tuple(args)
        | Type::Named { args, .. }
        | Type::Overload(args) => some(args, f),
        Type::Applied { ctor, args } => any_type(ctor, f) || some(args, f),
        Type::Array(t) | Type::ByName(t) | Type::Repeated(t) | Type::Annotated { tpe: t, .. } => {
            any_type(t, f)
        }
        Type::SingleType { prefix, .. } => any_type(prefix, f),
        Type::Function { params, ret } => some(params, f) || any_type(ret, f),
        Type::Method { paramss, ret } => paramss.iter().any(|ps| some(ps, f)) || any_type(ret, f),
        Type::BoundedWildcard { lo, hi } => {
            lo.as_ref().is_some_and(|t| any_type(t, f))
                || hi.as_ref().is_some_and(|t| any_type(t, f))
        }
        Type::Refined { parents, decls } => {
            some(parents, f)
                || decls.iter().any(|d| match d {
                    RefineDecl::Type { rhs, lo, hi, .. } => {
                        rhs.as_ref().is_some_and(|t| any_type(t, f))
                            || lo.as_ref().is_some_and(|t| any_type(t, f))
                            || hi.as_ref().is_some_and(|t| any_type(t, f))
                    }
                    RefineDecl::Def { paramss, ret, .. } => {
                        paramss.iter().any(|ps| some(ps, f)) || any_type(ret, f)
                    }
                    RefineDecl::Val { ty, .. } => any_type(ty, f),
                })
        }
        _ => false,
    }
}

/// Rewrite every node of `ty` bottom-up with `f`.
///
/// `f` sees each leaf, and each composite node once its children have already
/// been rewritten, so a rewrite that only replaces leaves needs no arm of its
/// own for the shape it is walking through.
pub(crate) fn map_type(ty: &Type, f: &mut impl FnMut(&Type) -> Type) -> Type {
    let rebuilt = match ty {
        Type::Class { sym, args } => Type::Class {
            sym: *sym,
            args: args.iter().map(|a| map_type(a, f)).collect(),
        },
        Type::Applied { ctor, args } => apply_type_ctor(
            map_type(ctor, f),
            args.iter().map(|a| map_type(a, f)).collect(),
        ),
        Type::Array(t) => Type::Array(Box::new(map_type(t, f))),
        Type::ByName(t) => Type::ByName(Box::new(map_type(t, f))),
        Type::Repeated(t) => Type::Repeated(Box::new(map_type(t, f))),
        Type::Tuple(ts) => Type::Tuple(ts.iter().map(|t| map_type(t, f)).collect()),
        Type::Overload(ts) => Type::Overload(ts.iter().map(|t| map_type(t, f)).collect()),
        Type::Named { name, args } => Type::Named {
            name: name.clone(),
            args: args.iter().map(|a| map_type(a, f)).collect(),
        },
        Type::Function { params, ret } => Type::Function {
            params: params.iter().map(|p| map_type(p, f)).collect(),
            ret: Box::new(map_type(ret, f)),
        },
        Type::Method { paramss, ret } => Type::Method {
            paramss: paramss
                .iter()
                .map(|ps| ps.iter().map(|p| map_type(p, f)).collect())
                .collect(),
            ret: Box::new(map_type(ret, f)),
        },
        Type::Annotated { tpe, annot } => Type::Annotated {
            tpe: Box::new(map_type(tpe, f)),
            annot: annot.clone(),
        },
        Type::BoundedWildcard { lo, hi } => Type::BoundedWildcard {
            lo: lo.as_ref().map(|t| Box::new(map_type(t, f))),
            hi: hi.as_ref().map(|t| Box::new(map_type(t, f))),
        },
        Type::SingleType { prefix, sym } => Type::SingleType {
            prefix: Box::new(map_type(prefix, f)),
            sym: *sym,
        },
        Type::Refined { parents, decls } => Type::Refined {
            parents: parents.iter().map(|p| map_type(p, f)).collect(),
            decls: decls.iter().map(|d| map_refine_decl(d, f)).collect(),
        },
        other => other.clone(),
    };
    f(&rebuilt)
}

fn map_refine_decl(d: &RefineDecl, f: &mut impl FnMut(&Type) -> Type) -> RefineDecl {
    match d {
        RefineDecl::Type {
            name,
            rhs,
            tparams,
            lo,
            hi,
        } => RefineDecl::Type {
            name: name.clone(),
            rhs: rhs.as_ref().map(|t| map_type(t, f)),
            tparams: *tparams,
            lo: lo.as_ref().map(|t| map_type(t, f)),
            hi: hi.as_ref().map(|t| map_type(t, f)),
        },
        RefineDecl::Def { name, paramss, ret } => RefineDecl::Def {
            name: name.clone(),
            paramss: paramss
                .iter()
                .map(|ps| ps.iter().map(|p| map_type(p, f)).collect())
                .collect(),
            ret: map_type(ret, f),
        },
        RefineDecl::Val { name, ty } => RefineDecl::Val {
            name: name.clone(),
            ty: map_type(ty, f),
        },
    }
}

/// Replace the abstract type member `m` with `to` throughout `ty`.
pub(crate) fn subst_type_member(ty: &Type, m: SymbolId, to: &Type) -> Type {
    let go = |t: &Type| subst_type_member(t, m, to);
    match ty {
        Type::TypeMember(id) if *id == m => to.clone(),
        Type::Class { sym, args } => Type::Class {
            sym: *sym,
            args: args.iter().map(go).collect(),
        },
        Type::Tuple(ts) => Type::Tuple(ts.iter().map(go).collect()),
        Type::Applied { ctor, args } => apply_type_ctor(go(ctor), args.iter().map(go).collect()),
        Type::Array(t) => Type::Array(Box::new(go(t))),
        Type::ByName(t) => Type::ByName(Box::new(go(t))),
        Type::Repeated(t) => Type::Repeated(Box::new(go(t))),
        Type::Annotated { tpe, annot } => Type::Annotated {
            tpe: Box::new(go(tpe)),
            annot: annot.clone(),
        },
        Type::Function { params, ret } => Type::Function {
            params: params.iter().map(go).collect(),
            ret: Box::new(go(ret)),
        },
        Type::Method { paramss, ret } => Type::Method {
            paramss: paramss
                .iter()
                .map(|ps| ps.iter().map(go).collect())
                .collect(),
            ret: Box::new(go(ret)),
        },
        _ => ty.clone(),
    }
}

/// Every abstract type member `ty` mentions, in order, without duplicates.
pub(crate) fn collect_type_members(ty: &Type, out: &mut Vec<SymbolId>) {
    match ty {
        Type::TypeMember(id) => {
            if !out.contains(id) {
                out.push(*id);
            }
        }
        Type::Class { args, .. } | Type::Tuple(args) | Type::Named { args, .. } => {
            for a in args {
                collect_type_members(a, out);
            }
        }
        Type::Applied { ctor, args } => {
            collect_type_members(ctor, out);
            for a in args {
                collect_type_members(a, out);
            }
        }
        Type::Array(t) | Type::ByName(t) | Type::Repeated(t) | Type::Annotated { tpe: t, .. } => {
            collect_type_members(t, out)
        }
        Type::Function { params, ret } => {
            for p in params {
                collect_type_members(p, out);
            }
            collect_type_members(ret, out);
        }
        Type::Method { paramss, ret } => {
            for ps in paramss {
                for p in ps {
                    collect_type_members(p, out);
                }
            }
            collect_type_members(ret, out);
        }
        _ => {}
    }
}

/// Replace `cls.this.type` with `to` throughout `ty`.
pub(crate) fn subst_this_type(ty: &Type, cls: SymbolId, to: &Type) -> Type {
    let go = |t: &Type| subst_this_type(t, cls, to);
    match ty {
        Type::ThisType(c) if *c == cls => to.clone(),
        Type::Class { sym, args } => Type::Class {
            sym: *sym,
            args: args.iter().map(go).collect(),
        },
        Type::Tuple(ts) => Type::Tuple(ts.iter().map(go).collect()),
        Type::Applied { ctor, args } => apply_type_ctor(go(ctor), args.iter().map(go).collect()),
        Type::Array(t) => Type::Array(Box::new(go(t))),
        Type::ByName(t) => Type::ByName(Box::new(go(t))),
        Type::Repeated(t) => Type::Repeated(Box::new(go(t))),
        Type::Annotated { tpe, annot } => Type::Annotated {
            tpe: Box::new(go(tpe)),
            annot: annot.clone(),
        },
        Type::Function { params, ret } => Type::Function {
            params: params.iter().map(go).collect(),
            ret: Box::new(go(ret)),
        },
        Type::Method { paramss, ret } => Type::Method {
            paramss: paramss
                .iter()
                .map(|ps| ps.iter().map(go).collect())
                .collect(),
            ret: Box::new(go(ret)),
        },
        // See `this_type_owners`. A view's *prefix* is left alone: it is
        // `SymbolTable::rewrite_view_this`'s, which has already run and
        // knows the difference between the receiver's `this` and its class.
        Type::SingleType { prefix, sym } => Type::SingleType {
            prefix: Box::new(go(prefix)),
            sym: *sym,
        },
        Type::Refined { parents, decls } => Type::Refined {
            parents: parents.iter().map(go).collect(),
            decls: decls
                .iter()
                .map(|d| match d {
                    RefineDecl::Type {
                        name,
                        rhs: Some(t),
                        tparams,
                        lo,
                        hi,
                    } if name != crate::prefix::PREFIX_MARK => RefineDecl::Type {
                        name: name.clone(),
                        rhs: Some(go(t)),
                        tparams: *tparams,
                        lo: lo.clone(),
                        hi: hi.clone(),
                    },
                    other => other.clone(),
                })
                .collect(),
        },
        _ => ty.clone(),
    }
}

/// Apply type arguments to a constructor (`Id` + `[A]` → `Id[A]`).
pub fn apply_type_ctor(ctor: Type, args: Vec<Type>) -> Type {
    if args.is_empty() {
        return ctor;
    }
    match ctor {
        Type::Class {
            sym,
            args: existing,
        } => {
            let mut all = existing;
            all.extend(args);
            Type::Class { sym, args: all }
        }
        Type::Named {
            name,
            args: existing,
        } => {
            let mut all = existing;
            all.extend(args);
            Type::Named { name, args: all }
        }
        Type::Applied {
            ctor,
            args: existing,
        } => {
            let mut all = existing;
            all.extend(args);
            apply_type_ctor(*ctor, all)
        }
        Type::Annotated { tpe, annot } => Type::Annotated {
            tpe: Box::new(apply_type_ctor(*tpe, args)),
            annot,
        },
        other => Type::Applied {
            ctor: Box::new(other),
            args,
        },
    }
}

/// A type argument that stands for a range rather than one type.
fn is_wildcard_arg(t: &Type) -> bool {
    matches!(t, Type::Wildcard | Type::BoundedWildcard { .. })
}

/// The right operand of `scala.Boolean.&&` or `scala.Boolean.||`, if `tree` is
/// one of those two applications.
///
/// nsc's `TailCalls` phase special-cases exactly these two symbols
/// (`Boolean_and` / `Boolean_or`) and transforms their argument *in the tail
/// context*, because both are compiled to a conditional branch over the
/// operand rather than to a call: nothing in the method runs after it. The
/// check is on the intrinsic, which is installed only on `scala.Boolean`, so a
/// user-defined `&&` on any other type is not affected.
pub fn bool_shortcircuit_rhs<'a>(
    st: &SymbolTable,
    tree: &'a scala_rs_parser::Tree,
) -> Option<&'a scala_rs_parser::Tree> {
    let scala_rs_parser::TreeKind::Apply { fun, args } = &tree.kind else {
        return None;
    };
    if args.len() != 1 {
        return None;
    }
    if fun.sym.is_none() {
        return None;
    }
    matches!(
        st.get(fun.sym).intrinsic,
        Intrinsic::BoolBin("&&") | Intrinsic::BoolBin("||")
    )
    .then(|| &args[0])
}

/// One base class's entry in a [`BaseTypeArgs`].
struct BaseSlot {
    /// The instantiation settled on, or — before the walk reaches this class —
    /// the first one a parent clause supplied.
    args: Vec<Type>,
    /// Whether the linearization has reached this class, which is the point at
    /// which every clause naming it has been seen and `more` can be merged in.
    settled: bool,
    /// Further instantiations the same class was reached at. Empty for every
    /// base class no two parent clauses disagree about, which is nearly all of
    /// them.
    more: Vec<Vec<Type>>,
}

/// nsc's `BaseTypeSeq`: every base class of an applied class, mapped to the
/// type arguments it is instantiated at there. Built by
/// [`SymbolTable::base_type_args`].
#[derive(Default)]
pub(crate) struct BaseTypeArgs {
    slots: rustc_hash::FxHashMap<u32, BaseSlot>,
}

impl BaseTypeArgs {
    /// The arguments `sym` is seen at, or `None` when it is not a base class.
    pub(crate) fn get(&self, sym: &u32) -> Option<&Vec<Type>> {
        self.slots.get(sym).map(|s| &s.args)
    }
}

/// Memo for [`SymbolTable::class_derives_from`], scoped to one base-type
/// walk: `(sub, sup, answer)`, scanned linearly because it holds a handful of
/// entries.
type AncMemo = Vec<(u32, u32, bool)>;
