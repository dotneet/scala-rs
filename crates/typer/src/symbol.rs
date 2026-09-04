//! Symbols, scopes, and the compilation context.

use scala_rs_parser::{Flags, RefineDecl, SymbolId, Type};

/// Decl name that marks a refinement as the as-seen-from view of a type
/// projection rather than something the program wrote. Not a legal Scala
/// identifier, so it can never collide with a real member.
pub const AS_SEEN_FROM_MARK: &str = "<asSeenFrom>";

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

struct BoundGuard(bool);

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
            return None;
        }
        v.push(id);
        Some(AliasGuard)
    })
}

/// Returns `None` once the parent walk is implausibly deep, which only
/// happens when the hierarchy has a cycle.
fn enter_depth() -> Option<BoundGuard> {
    EXPANDING_BOUNDS.with(|b| {
        let mut v = b.borrow_mut();
        if v.len() > 200 {
            return None;
        }
        v.push(u32::MAX);
        Some(BoundGuard(true))
    })
}

/// Returns `None` when this parameter's bound is already being expanded.
fn enter_bound(id: SymbolId) -> Option<BoundGuard> {
    EXPANDING_BOUNDS.with(|b| {
        let mut v = b.borrow_mut();
        if v.contains(&id.0) {
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MacroBinding {
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
    pub intrinsic: Intrinsic,
    /// Constructor / method parameter symbols (flat, first clause).
    pub params: Vec<SymbolId>,
    pub paramss: Vec<Vec<SymbolId>>,
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
    /// Language annotations (`@deprecated(...)`, `@tailrec`, …) copied from modifiers.
    pub annotations: Vec<scala_rs_parser::Tree>,
    /// Lower bound of an abstract/HK type member (`type F[_] >: Lo`).
    pub bound_lo: Option<Type>,
    /// Upper bound of an abstract/HK type member (`type F[_] <: Hi`).
    pub bound_hi: Option<Type>,
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
    /// nsc `ABSOVERRIDE`: the source wrote `abstract override`, so `super` in
    /// this member is bound by the *linearization* of whatever concrete class
    /// mixes the trait in. `flags` cannot carry this: the namer already sets
    /// `ABSTRACT` on every body-less `def`, so `override def close(): Unit`
    /// (deferred) and `abstract override def close(): Unit = …` (stackable)
    /// are indistinguishable there.
    pub abstract_override: bool,
    /// nsc `DEFERRED` for a **value**: `val v: Int` / `var v: Int` written with
    /// no right-hand side. The namer sets `ABSTRACT` on a body-less `def` but
    /// not on a body-less `val`, so without this an abstract `val` in a trait
    /// looks exactly like a concrete one and `class C extends T` cannot tell
    /// whether `v` still needs implementing.
    pub deferred_val: bool,
}

impl Symbol {
    pub fn is_class_like(&self) -> bool {
        matches!(self.kind, SymKind::Class | SymKind::ModuleClass)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Scope {
    map: HashMap<String, Vec<SymbolId>>,
    /// Owners brought in by a wildcard import (`import p._`) in this scope,
    /// with the names that selector hid (`import p.{X => _, _}`).
    /// A package read from a jar cannot be enumerated up front, so the names
    /// it offers are resolved on demand: see `Checker::expose_unqualified`.
    wildcards: Vec<WildcardImport>,
}

/// `import owner._`, minus the names hidden by `X => _` selectors.
#[derive(Clone, Debug)]
pub struct WildcardImport {
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
        let slot = self.map.entry(name.to_string()).or_default();
        // One symbol reachable by two routes is still one symbol, not an
        // overload: a template's self alias, for instance, is entered both
        // with the rest of the class's members and by `bind_self_type`.
        if slot.contains(&id) {
            return;
        }
        slot.push(id);
    }

    pub fn enter_wildcard(&mut self, owner: SymbolId, hidden: &[String]) {
        if let Some(w) = self.wildcards.iter_mut().find(|w| w.owner == owner) {
            w.hidden.retain(|h| hidden.iter().any(|n| n == h));
            return;
        }
        self.wildcards.push(WildcardImport {
            owner,
            hidden: hidden.to_vec(),
        });
    }

    pub fn wildcards(&self) -> &[WildcardImport] {
        &self.wildcards
    }

    pub fn lookup(&self, name: &str) -> &[SymbolId] {
        self.map.get(name).map(|v| v.as_slice()).unwrap_or(&[])
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
    pub fn entries(&self) -> impl Iterator<Item = (&String, &[SymbolId])> {
        self.map.iter().map(|(k, v)| (k, v.as_slice()))
    }
}

pub struct SymbolTable {
    pub symbols: Vec<Symbol>,
    pub scopes: Vec<Scope>,
    pub root: SymbolId,
    pub scala_pkg: SymbolId,
    pub predef: SymbolId,
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
    /// Terms whose pre-erasure type was a user value class, and which one.
    /// Erasure replaces the type with the underlying representation, but the
    /// backend still has to know that `case class Box(m: Meters)` prints its
    /// field as a boxed `Meters`.
    pub value_class_terms: rustc_hash::FxHashMap<SymbolId, SymbolId>,
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
    /// One past the last symbol `install_prelude` built.
    ///
    /// The prelude hand-writes signatures for the part of `scala.*` the typer
    /// reasons about, and those must never be reshaped from a jar. Everything
    /// `scala.*` the prelude does *not* cover (`scala.concurrent.Future`, for
    /// one) arrives from a classfile instead, where a by-name parameter is
    /// indistinguishable from a `Function0`. This is the line between the two.
    pub prelude_end: u32,
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
            symbols: vec![Symbol {
                id: SymbolId(0),
                name: "<none>".into(),
                owner: SymbolId(0),
                kind: SymKind::NoSymbol,
                flags: Flags::EMPTY,
                ty: Type::NoType,
                members: vec![],
                jvm_name: String::new(),
                intrinsic: Intrinsic::None,
                params: vec![],
                paramss: vec![],
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
                annotations: vec![],
                bound_lo: None,
                bound_hi: None,
                captures: vec![],
                macro_impl: None,
                declaring_class: String::new(),
                declaring_is_interface: false,
                pickled_origin: String::new(),
                abstract_override: false,
                deferred_val: false,
            }],
            scopes: vec![Scope::default()],
            root: SymbolId(0),
            scala_pkg: SymbolId(0),
            predef: SymbolId(0),
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
            value_class_terms: rustc_hash::FxHashMap::default(),
            erased_abstract_params: rustc_hash::FxHashMap::default(),
            source_value_classes: rustc_hash::FxHashSet::default(),
            source_classes: rustc_hash::FxHashSet::default(),
            lazy_cells: Vec::new(),
            local_lazy_cells: rustc_hash::FxHashSet::default(),
            local_lazy_accessors: rustc_hash::FxHashMap::default(),
            local_lazy_nlr: rustc_hash::FxHashSet::default(),
            prelude_end: 0,
            seq_extractor_payload: rustc_hash::FxHashMap::default(),
            jvm_index: std::cell::RefCell::new(JvmIndex::default()),
            erasure_settled: false,
            flattened_upto: 0,
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
            intrinsic: Intrinsic::None,
            params: vec![],
            paramss: vec![],
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
            annotations: vec![],
            bound_lo: None,
            bound_hi: None,
            captures: vec![],
            macro_impl: None,
            declaring_class: String::new(),
            declaring_is_interface: false,
            pickled_origin: String::new(),
            abstract_override: false,
            deferred_val: false,
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
        &mut self.symbols[id.0 as usize]
    }

    pub fn enter_in_current(&mut self, name: &str, id: SymbolId) {
        self.scopes.last_mut().unwrap().enter(name, id);
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
                return found.to_vec();
            }
        }
        Vec::new()
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
            let found = sc.lookup(name);
            if found.is_empty() {
                continue;
            }
            if found.iter().any(|&s| self.is_type_namespace(s)) {
                return found.to_vec();
            }
            if module_fallback.is_empty() && found.iter().any(|&s| self.is_module_like(s)) {
                module_fallback = found.to_vec();
            }
        }
        module_fallback
    }

    /// Look a name up in the *term* namespace, the mirror of `lookup_type`.
    /// `import syntax._` bringing a `type HNil` alias into scope must not hide
    /// the top-level `object HNil` from `HNil.type`, so a scope that binds the
    /// name only in the type namespace is skipped and the search continues
    /// outward.
    pub fn lookup_term(&self, name: &str) -> Vec<SymbolId> {
        for sc in self.scopes.iter().rev() {
            let found = sc.lookup(name);
            if found.is_empty() {
                continue;
            }
            if found.iter().any(|&s| self.is_term_namespace(s)) {
                return found.to_vec();
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
            let found: Vec<SymbolId> = sc
                .lookup(name)
                .iter()
                .copied()
                .filter(|&s| self.get(s).kind != SymKind::Method)
                .collect();
            if !found.is_empty() {
                return found;
            }
        }
        Vec::new()
    }

    /// Names that live in the term namespace under their own spelling.
    fn is_term_namespace(&self, s: SymbolId) -> bool {
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

    pub fn lookup_member(&self, owner: SymbolId, name: &str) -> Vec<SymbolId> {
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
                if let Some(ps) = self.class_sym_of(st) {
                    work.push(ps);
                }
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
    pub fn widen_type_param(&self, ty: &Type) -> Type {
        let mut t = ty.clone();
        for _ in 0..8 {
            match &t {
                Type::TypeParam(id) => match self.get(*id).bound_hi.clone() {
                    Some(hi) => t = hi,
                    None => return ty.clone(),
                },
                Type::Applied { ctor, args } => {
                    let Type::TypeParam(id) = ctor.as_ref() else {
                        break;
                    };
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
        match self.get(sym).ty.clone() {
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
            Type::AnyRef => Some(self.anyref_sym),
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
            // resolves through its bound, as in nsc.
            Type::TypeParam(id) => match &self.get(*id).bound_hi {
                Some(hi) => self.class_sym_of(hi),
                None => Some(self.any_sym),
            },
            Type::Applied { ctor, .. } => self.class_sym_of(ctor),
            Type::TypeMember(id) => {
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
            Type::Wildcard | Type::BoundedWildcard { .. } => Some(self.any_sym),
            Type::ThisType(sym) => Some(*sym),
            Type::Constant(lit) => self.class_sym_of(&Type::lit_underlying(lit)),
            Type::SingleType { prefix, sym } => {
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
            Type::Tuple(ts) if !ts.is_empty() => self
                .lookup(&format!("Tuple{}", ts.len()))
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

    pub fn module_class_of(&self, id: SymbolId) -> SymbolId {
        match self.get(id).ty {
            Type::ModuleRef(c) => c,
            _ => id,
        }
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
        subst_tparams_cow(&self.get(owner).tparams, args, ty)
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
    pub fn type_member_as_seen(&self, id: SymbolId) -> Type {
        if !self.get(id).tparams.is_empty() {
            Type::TypeMember(id)
        } else {
            match self.get(id).ty.clone() {
                Type::NoType | Type::Error | Type::TypeMember(_) => Type::TypeMember(id),
                other => other,
            }
        }
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
                        return self.subst_tparams(*id, args, &info.ty);
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
        let eta_ok = |t: &Type| self.hk_alias(t).is_some() || matches!(t, Type::Class { .. });
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

    /// Conformance between two type constructors, decided on their bodies.
    /// See [`SymbolTable::eta_expand_pair`].
    fn hk_alias_sub_type(&self, a: &Type, b: &Type) -> Option<bool> {
        let (ea, eb) = self.eta_expand_pair(a, b)?;
        Some(self.is_sub_type(&ea, &eb))
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
    fn this_type_owners(ty: &Type, out: &mut Vec<SymbolId>) {
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
            _ => {}
        }
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
                if let Some(ps) = self.class_sym_of(st) {
                    work.push(ps);
                }
            }
        }
        false
    }

    /// Substitute inherited member types using applied parents (`Functor[Id].map`).
    pub fn subst_as_seen_from(&self, recv: &Type, ty: &Type) -> Type {
        // `this.type` in a member's signature means the receiver it was
        // selected on: `def add(v: T): this.type` on a `B[String]` gives back a
        // `B[String]`, not a bare `B` whose argument has to be invented.
        let ty = &{
            let mut owners = Vec::new();
            Self::this_type_owners(ty, &mut owners);
            let mut out = ty.clone();
            if !owners.is_empty() && !matches!(recv, Type::ThisType(_)) {
                if let Some(rc) = self.class_sym_of(recv) {
                    for c in owners {
                        if c == rc || self.is_ancestor_of(c, rc) {
                            out = subst_this_type(&out, c, recv);
                        }
                    }
                }
            }
            out
        };
        fn walk(
            st: &SymbolTable,
            recv: &Type,
            ty: Type,
            seen: &mut rustc_hash::FxHashSet<u32>,
        ) -> Type {
            match recv {
                Type::Class { sym, args } => {
                    if !seen.insert(sym.0) {
                        return ty;
                    }
                    let mut t = if args.is_empty() {
                        ty
                    } else {
                        st.subst_tparams(*sym, args, &ty)
                    };
                    for p in &st.get(*sym).parents {
                        // The parent is declared in terms of *this* class's
                        // type parameters, so it has to be instantiated before
                        // it can instantiate anything itself. Without this,
                        // `OptionMapper2[B1, B2, Boolean, P1, P2, R].column`
                        // keeps its `implicit TypedType[BR]` raw instead of
                        // resolving `BR` to `Boolean` through
                        // `OptionMapper[BR, R]`.
                        let p = st.subst_tparams_cow(*sym, args, p);
                        t = walk(st, &p, t, seen);
                    }
                    // A self type is a second place `this` inherits members
                    // from, and they are declared in *its* vocabulary:
                    // `trait P[A] { self: Q[A] => def p: A = q }` reads `q`
                    // out of `Q`, whose own `A` has to become `P`'s. Without
                    // this the two `A`s printed the same and compared
                    // unequal -- "type mismatch; found: A required: A".
                    if let Some(sf) = &st.get(*sym).self_type {
                        let sf = st.subst_tparams_cow(*sym, args, sf);
                        t = walk(st, &sf, t, seen);
                    }
                    t
                }
                Type::ModuleRef(sym) => {
                    if !seen.insert(sym.0) {
                        return ty;
                    }
                    let mut t = ty;
                    for p in &st.get(*sym).parents {
                        t = walk(st, p, t, seen);
                    }
                    t
                }
                Type::Annotated { tpe, .. } => walk(st, tpe, ty, seen),
                // `trait C[-T] extends (T => R)` inherits `Function1.apply`,
                // and reading its type through `C[X]` means walking into
                // `Function1[X, R]`. A structural function names no class, so
                // it has to be read back as one first.
                Type::Function { .. } => match st.function_class_form(recv) {
                    Some(c) => walk(st, &c, ty, seen),
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
                    walk(st, &Type::Class { sym: *sym, args }, ty, seen)
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
                    walk(st, &t, ty, seen)
                }
                // A member reached through `Ops[F, A] { type TypeClassType =
                // FlatMap[F] }` is declared by one of the parents and has to be
                // read at that parent's arguments. Without this, cats' whole
                // syntax layer -- every result type simulacrum writes is a
                // refinement -- handed back `flatMap`'s raw `A`.
                Type::Refined { parents, .. } => {
                    let mut t = ty;
                    for p in parents {
                        t = walk(st, p, t, seen);
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
                        Some(hi) => walk(st, &hi, ty, seen),
                        None => ty,
                    }
                }
                _ => ty,
            }
        }
        let mut seen = rustc_hash::FxHashSet::default();
        walk(self, recv, ty.clone(), &mut seen)
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
                self.symbols[id.0 as usize].jvm_name == jvm && !self.is_primitive_value_class(id)
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
                let p = subst_tparams_cow(&s.tparams, args, p);
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
                            self.lub(x, y)
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
                                hi: Some(Box::new(self.lub(x, y))),
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
                    ret: Box::new(self.lub(r1, r2)),
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
                let joined = self.lub(&cand, other);
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

    pub fn is_sub_type(&self, a: &Type, b: &Type) -> bool {
        if a == b {
            return true;
        }
        // `A#B` carries what `A` settles as a refinement so member reads can
        // see it, but it *is* `B`: the same class reached by an alias
        // (`type Session = JdbcSessionDef`) carries nothing, and slick passes
        // the two to each other constantly. Constraining either direction
        // would invent errors nsc does not have.
        if let Some(p) = Self::as_seen_from_view(a) {
            return self.is_sub_type(p, b);
        }
        if let Some(p) = Self::as_seen_from_view(b) {
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
                        } else if is_wildcard_arg(x) || is_wildcard_arg(y) {
                            // An invariant parameter still *contains* a
                            // wildcard: `List[Byte]` is a
                            // `Collection[_ <: Number]`.
                            self.is_sub_type(x, y)
                        } else {
                            // Invariant: `A[Int]` is not an `A[Any]`.
                            self.is_sub_type(x, y) && self.is_sub_type(y, x)
                        }
                    })
                } else {
                    false
                }
            }
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
                self.is_sub_type(c1, c2)
                    && a1
                        .iter()
                        .zip(a2.iter())
                        .all(|(x, y)| self.is_sub_type(x, y))
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
                    if let Type::TypeMember(id) = ctor.as_ref() {
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
                            let hi = self.subst_tparams(*id, &args, &hi);
                            return self.is_sub_type(&hi, other);
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
            // Annotations are erased for conformance: `Node` is a
            // `Node @uncheckedVariance`. Like the wildcards below, this has to
            // come before the Class-parent walk, which matches every `Class`
            // on the left whatever `b` is and would answer "no" by running out
            // of parents.
            (a, Type::Annotated { tpe, .. }) => self.is_sub_type(a, tpe),
            (Type::Annotated { tpe, .. }, b) => self.is_sub_type(tpe, b),
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
            (Type::String, b) => {
                let Some(_g) = enter_depth() else {
                    return false;
                };
                let parents = &self.get(self.string_sym).parents;
                parents.iter().any(|p| self.is_sub_type(p, b))
            }
            (Type::Class { sym: s1, args: a1 }, b) => {
                // `scala.FunctionN[T1, …, R]` and the structural function type
                // are one and the same type; the prelude writes a parent that
                // *is* a function (`PartialFunction`, `Map`) as the class and
                // everything else as the structural form.
                if let Some(f) = self.function_class_shape(*s1, a1) {
                    return self.is_sub_type(&f, b);
                }
                // A malformed hierarchy (`object B extends B`) would otherwise
                // walk its own parents forever. Depth, not identity: a legitimate
                // walk revisits a class at a different type argument.
                let Some(_g) = enter_depth() else {
                    return false;
                };
                // Borrowed: this is the arm the subtype walk spends most of
                // its time in, and cloning the parent list and the type
                // parameters at every node of the DAG dominated its cost.
                let child = self.get(*s1);
                child.parents.iter().any(|p| {
                    let p = subst_tparams_cow(&child.tparams, a1, p);
                    self.is_sub_type(&p, b)
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
                if is_wildcard_arg(x) || is_wildcard_arg(y) {
                    self.is_sub_type(x, y)
                } else {
                    self.is_sub_type(x, y) && self.is_sub_type(y, x)
                }
            }
            (Type::ModuleRef(s), Type::Class { sym, .. }) if s == sym => true,
            (Type::ModuleRef(s), b) => {
                let Some(_g) = enter_depth() else {
                    return false;
                };
                self.get(*s).parents.iter().any(|p| self.is_sub_type(p, b))
            }
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
                p2.iter()
                    .zip(p1.iter())
                    .all(|(exp, act)| self.is_sub_type(exp, act))
                    && self.is_sub_type(r1, r2)
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

    pub fn display_type(&self, ty: &Type) -> String {
        // The as-seen-from view of `A#B` prints as `B`: its decls are the
        // compiler's bookkeeping, not something the program wrote.
        if let Some(p) = Self::as_seen_from_view(ty) {
            return self.display_type(p);
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
            Type::TypeParam(id) => self.get(*id).name.clone(),
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
                let name = self.get(*id).name.clone();
                // `from` first, then its lexically enclosing classes: an inner
                // class (`Main.factory: Main.Factory`) sees `Main`'s implementation
                // of an abstract member declared beside `Factory`, which is what
                // nsc reaches through the outer-instance prefix.
                for owner in self.enclosing_classes(from) {
                    for m in self.lookup_member(owner, &name) {
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
        let cls = self.class_sym_of(ty)?;
        let jvm = self.get(cls).jvm_name.clone();
        if jvm.starts_with("scala/Function") || jvm.ends_with("PartialFunction") {
            return None;
        }
        let abstracts = self.abstract_sam_methods(cls);
        if abstracts.len() != 1 {
            return None;
        }
        let method = abstracts[0];
        // The abstract method may be declared in a *parent* (`trait C[-T]
        // extends (T => R)` gets its `apply` from `Function1`), so its type has
        // to be read as seen from `ty` -- substituting only `cls`'s own type
        // parameters leaves the parent's untouched.
        let recv = match ty {
            Type::Class { .. } => ty.clone(),
            _ => Type::Class {
                sym: cls,
                args: Vec::new(),
            },
        };
        let subst = |t: &Type| self.subst_as_seen_from(&recv, t);
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
            .filter(|m| self.get(*m).flags.contains(Flags::ABSTRACT))
            .collect()
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
                            _ => subst_refine_aliases(st, decls, rhs),
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
                .map(|a| subst_refine_aliases(st, decls, a))
                .collect(),
        },
        Type::Applied { ctor, args } => {
            let applied = apply_type_ctor(
                subst_refine_aliases(st, decls, ctor),
                args.iter()
                    .map(|a| subst_refine_aliases(st, decls, a))
                    .collect(),
            );
            st.expand_applied_hk_alias(applied)
        }
        Type::Array(t) => Type::Array(Box::new(subst_refine_aliases(st, decls, t))),
        Type::Function { params, ret } => Type::Function {
            params: params
                .iter()
                .map(|p| subst_refine_aliases(st, decls, p))
                .collect(),
            ret: Box::new(subst_refine_aliases(st, decls, ret)),
        },
        Type::Method { paramss, ret } => Type::Method {
            paramss: paramss
                .iter()
                .map(|ps| {
                    ps.iter()
                        .map(|p| subst_refine_aliases(st, decls, p))
                        .collect()
                })
                .collect(),
            ret: Box::new(subst_refine_aliases(st, decls, ret)),
        },
        Type::ByName(t) => Type::ByName(Box::new(subst_refine_aliases(st, decls, t))),
        Type::Repeated(t) => Type::Repeated(Box::new(subst_refine_aliases(st, decls, t))),
        Type::Tuple(ts) => Type::Tuple(
            ts.iter()
                .map(|t| subst_refine_aliases(st, decls, t))
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
fn subst_this_type(ty: &Type, cls: SymbolId, to: &Type) -> Type {
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
