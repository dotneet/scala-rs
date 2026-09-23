//! Complete standard-library members from their `ScalaSignature` pickles, on
//! demand, when the hand-written prelude does not have them.
//!
//! `prelude.rs` declares library members by hand. That does not scale to a
//! 2.13-compatible surface, so this module fills the gaps: when member
//! resolution on a `scala.*` receiver fails outright, the receiver's pickle is
//! read (and its parents', through `scala-rs-pickle`'s `SigCache`) and the
//! missing member is installed on the receiver's class symbol.
//!
//! Three rules keep it honest:
//!
//! 1. **The prelude always wins.** This runs only when `lookup_member` found
//!    *nothing*, so a hand-written declaration is never shadowed or replaced.
//! 2. **A member we cannot express is not supplied.** If the pickled type does
//!    not map onto `scala_rs_parser::Type`, or if we cannot pin down the erased
//!    JVM descriptor to call it with, the member is skipped and the user gets
//!    the usual "is not a member" error. A wrong type would be worse than none.
//! 3. **Nothing is read ahead of time.** One classfile per (receiver, name)
//!    miss, cached.

use std::collections::{HashMap, HashSet};

use scala_rs_parser::{Flags, SymbolId, Type};
use scala_rs_pickle::read::pflags;
use scala_rs_pickle::sym::{MacroImpl as PickledMacroImpl, MemberKind, SigCache, SigType};
use scala_rs_pickle::ClassSource;

use crate::javaclass::{parse_java_classfile, BinaryIndex, JavaClass};
use crate::symbol::{MacroBinding, MacroTarg, SymKind, SymbolTable};

/// `SCALA_RS_PICKLE_DEBUG=1` traces why a member was or was not supplied.
/// Completion is silent otherwise: a member it declines to supply surfaces as
/// the typer's ordinary "is not a member".
/// Whether a converted method result is an abstract type member, applied or
/// not (`Repr[A]`, `Elem`).
fn is_type_member_result(ret: &Type) -> bool {
    match ret {
        Type::TypeMember(_) => true,
        Type::Applied { ctor, .. } => matches!(**ctor, Type::TypeMember(_)),
        _ => false,
    }
}

pub(crate) fn trace(args: std::fmt::Arguments<'_>) {
    // Read once. This is called from the middle of member completion, and
    // `var_os` walks the process environment on every call.
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *ON.get_or_init(|| std::env::var_os("SCALA_RS_PICKLE_DEBUG").is_some()) {
        eprintln!("[pickle] {args}");
    }
}

/// One JVM method declaration matching a pickled member.
struct ErasedDecl {
    /// The erased descriptor to call it with.
    desc: String,
    /// JVM internal name of the class file that declares it.
    declared_in: String,
    /// Whether that class file is an interface, and so whether the call is
    /// `invokeinterface` or `invokevirtual`.
    declared_by_interface: bool,
    /// True when the search reached `declared_in` only through a *pickled*
    /// parent -- a hop the JVM cannot make, because the class file that named
    /// it declares no parents at all (`api/JavaUniverse` has `interfaces: 0`
    /// even though the trait extends the abstract class `Universe`).
    ///
    /// The call then cannot name the receiver's class: resolution would look
    /// only where the bytecode leads and find nothing. It has to name
    /// `declared_in`, with the receiver cast to it first.
    off_the_bytecode_path: bool,
}

/// The converted type member plus the declaration that supplied it. A
/// transparent (nullary) alias has no source-level `TypeMember` in the type
/// tree, but dependent selections still need that declaration's owner/path so
/// the call-site prefix can be substituted without consulting lookup order.
struct CompletedTypeMember {
    ty: Type,
    decl: Option<SymbolId>,
}

const ACC_STATIC: u16 = 0x0008;
const ACC_BRIDGE: u16 = 0x0040;
const ACC_SYNTHETIC: u16 = 0x1000;

/// A `ClassSource` view of the typer's `BinaryIndex`.
struct BinSource<'a>(&'a mut BinaryIndex);

impl ClassSource for BinSource<'_> {
    fn class_bytes(&mut self, internal_name: &str) -> Option<Vec<u8>> {
        self.0.find_class(internal_name).ok().flatten()
    }
}

#[derive(Default)]
pub struct PickleSupply {
    sigs: SigCache,
    /// `(receiver class, member name)` pairs already attempted, so a miss
    /// costs one lookup, not one per mention.
    tried: HashSet<(u32, String)>,
    /// Parsed classfiles, for erased descriptors.
    classes: HashMap<String, Option<JavaClass>>,
    /// Library classes stubbed into the symbol table by `ensure_class`, keyed
    /// by JVM internal name, so a second mention reuses the same symbol.
    stubs: HashMap<String, SymbolId>,
    /// Classes whose pickled parents have already been attached.
    parented: HashSet<u32>,
    /// Dotted names `conv` needed and could not turn into a symbol, since the
    /// last `take_unresolved_refs`. The typer uses them to decide which
    /// classfiles to load before trying again (see `Check::pickled_alias_type`).
    unresolved_refs: Vec<String>,
    /// What `this.type` means while a member of one class is being installed:
    /// the receiver applied to its own type parameters. `Growable.addOne` and
    /// `+=` / `++=` all return `this.type`, and for a
    /// `Builder[Int, List[Int]]` receiver that is a `Builder`, not the
    /// `Growable` the erased signature names.
    self_ty: Option<Type>,
    /// The class a type member is being completed **for**, when that is not
    /// the class that declares it.
    ///
    /// `slick.basic.BasicProfile.API` declares `type Session = Backend#Session`
    /// and leaves `Backend` abstract; `slick.jdbc.JdbcProfile`, which is
    /// where a concrete profile's `api` object really lives, fixes `type
    /// Backend = JdbcBackend`. The right-hand side has to be read in the
    /// vocabulary of the class the name was asked of, or the projection's
    /// prefix never resolves to anything but the abstract declaration.
    completing_for: Option<SymbolId>,
    /// What a `p.type` naming one of the member's *own* parameters means,
    /// while that member is being installed: the parameter's declared type.
    ///
    /// Keyed by the pickled parameter's complete path
    /// (`cats.effect.kernel.Async.apply.F`). A suffix-only key lets two
    /// same-shaped value parameters race through a `HashMap`; the pickle
    /// reader preserves this path for dependent `TypeRef`s.
    param_singletons: HashMap<String, Type>,
    /// The term symbol for each parameter whose singleton is being widened.
    /// A dependent result such as `pShape.Packed` must retain the formal
    /// parameter path until the call supplies `pShape`; the widened type alone
    /// cannot be substituted back to the actual argument.
    param_singleton_symbols: HashMap<String, SymbolId>,
    /// Classes read from a `-cp` jar/directory whose shape has been taken from
    /// their `ScalaSignature` rather than from the JVM generic signature (see
    /// [`PickleSupply::adopt_binary_class`]). Also the gate that lets
    /// `complete_named` serve a class outside `scala.*`.
    adopted: HashSet<u32>,
    /// Classes [`PickleSupply::supply_implicit_members`] has already served.
    implicits_supplied: HashSet<u32>,
    /// `(class, selected member)` pairs whose relevant implicit conversions
    /// have already been inspected for an extension search. The conversion
    /// declaration has a different name from the selected member (`when` is
    /// supplied by `convertToWordSpecStringWrapper`), so this cache belongs
    /// beside the pickle lookup rather than the ordinary member-name cache.
    implicit_extensions_checked: HashSet<(u32, String)>,
    /// What [`PickleSupply::implicit_member_names`] answered for each class.
    implicit_names: HashMap<u32, Vec<String>>,
    /// What [`PickleSupply::concrete_method_names`] answered for each class.
    concrete_names: HashMap<u32, Vec<String>>,
    /// Source-level member names declared in a class's pickle, including vals
    /// and nested types. Used to tell whether wildcard-importing a binary
    /// module needs its pickle adopted to expose members the classfile reader
    /// has not installed yet.
    pickled_member_names: HashMap<u32, Vec<String>>,
    /// The declaration-side stable prefix attached to a pickled value/method
    /// result. A Scala pickle cannot put `C.this` in the `SigType` of a
    /// method result, so `BasicProfile.API#Database` stores the type as
    /// `BasicBackend.DatabaseFactory` and carries
    /// `BasicProfile.this.backend` here. The prefix must survive installation
    /// until member selection can map it to the concrete receiver.
    result_prefixes: HashMap<SymbolId, SigType>,
    /// A pickled method result spelled as a type member of its declaring
    /// `this` prefix, retained before conversion widens the abstract member to
    /// its upper bound. Member selection re-applies the concrete receiver's
    /// override (for example `Repr[O]`) after lazy alias completion.
    result_type_member_apps: HashMap<SymbolId, (String, Vec<Type>)>,
    /// What [`PickleSupply::complete_type_member`] answered for each
    /// `(class, name)`, so a miss costs one pickle walk and a hit stays
    /// stable. A memo of the *answer*, not just of having asked: a nullary
    /// alias installs no symbol of its own, so re-deriving it is the only way
    /// to answer twice, and "already tried" would make the second mention of
    /// `c.Tree` in one signature fail.
    ///
    /// Separate from `tried`, which is the term namespace: a class can have
    /// both a `type Expr` and a `val Expr`, and asking for one must not make
    /// the other look already-answered.
    tried_types: HashMap<(u32, String), Option<Type>>,
    /// The declaration selected while answering `tried_types`. This is kept
    /// separately because transparent aliases memoize their concrete RHS,
    /// while a dependent projection must retain the declaration origin.
    completed_type_member_decls: HashMap<(u32, String), SymbolId>,
    /// Pickled `val`s whose singleton type is being widened right now, so
    /// `val x: y.type; val y: x.type` cannot spiral.
    widening: HashSet<String>,
    /// While set, [`PickleSupply::self_type_member`] resolves a bare type name
    /// the way the class that *declares* the member sees it, ignoring a
    /// concrete alias a more derived class in the linearisation gives the same
    /// name. That is the vocabulary the JVM descriptor was erased in, and the
    /// only thing this is used for is recovering that descriptor -- see the
    /// declaration-site retry in [`PickleSupply::install`].
    decl_site_erasure: bool,
    /// Set once a macro def has been supplied from a jar's pickle. The typer
    /// copies it into `Check::has_macro_defs`, which is the gate on walking
    /// every typed application looking for something to expand; a run that
    /// defines no macro and calls none must not pay for that walk.
    pub supplied_macro_def: bool,
}

/// One `type T[...] = U` recovered from a package object's pickle.
///
/// Package-object aliases exist *only* in the pickle: `<pkg>/package$.class`
/// declares no member for them, so reading the classfile leaves them behind.
#[derive(Clone, Debug)]
pub struct PickledAlias {
    pub name: String,
    pub tparams: Vec<PickledTParam>,
    /// The right-hand side, still in the alias's own vocabulary.
    pub rhs: SigType,
}

/// A type parameter of a pickled alias. `arity` is its own kind arity, so
/// `type Ref[F[_], A]` yields `[("F", 1), ("A", 0)]`.
#[derive(Clone, Debug)]
pub struct PickledTParam {
    pub name: String,
    pub arity: usize,
}

impl PickleSupply {
    pub fn new() -> Self {
        PickleSupply::default()
    }

    /// Declaration-side outer path recorded for an installed pickled member.
    /// The typer uses it to rebind a result such as
    /// `BasicProfile.this.backend.DatabaseFactory` to the concrete profile
    /// that supplied the selected API.
    pub(crate) fn result_prefix_for(&self, member: SymbolId) -> Option<&SigType> {
        self.result_prefixes.get(&member)
    }

    pub(crate) fn result_type_member_app(&self, member: SymbolId) -> Option<&(String, Vec<Type>)> {
        self.result_type_member_apps.get(&member)
    }

    /// Try to install `name` on `class_sym` from the library pickles.
    /// Returns true if at least one member was installed.
    /// Try to install `name` for a receiver whose class symbol is `class_sym`.
    ///
    /// Returns the symbols it installed, and on which owner: a member found on
    /// the companion object lands on the companion's module class, not on
    /// `class_sym`, so the caller has to look there too.
    pub fn complete(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        class_sym: SymbolId,
        name: &str,
    ) -> Vec<SymbolId> {
        let mut out = self.complete_on_class(st, bin, class_sym, name);
        // `Iterator.from(1)`: the prelude has the trait but no companion, so
        // the receiver resolved to the class. The member lives on the
        // companion object, which is where it has to be installed -- putting
        // it on the trait would emit an invokevirtual against a method that is
        // not there. codegen already loads `X$.MODULE$` when a method's owner
        // is a module class, so this comes out right.
        //
        // The companion is consulted even when the class itself supplied
        // something, and the two results are unioned. Gating it on "the class
        // supplied nothing" made the answer depend on unrelated global state:
        // `scala.math.BigDecimal` declares an instance `apply(MathContext)`,
        // whose parameter `conv` can only map once *some other* code has
        // pulled `java.math.MathContext` into the symbol table. So
        // `BigDecimal(2)` resolved against the companion's seven `apply`
        // overloads on its own, but against that single instance `apply` in a
        // unit that had already mentioned `java.math.BigDecimal` -- the same
        // program compiled or not depending on statement order. Completion is
        // additive by contract; a class-side hit must not hide the
        // companion's.
        if !class_sym.is_none() && st.get(class_sym).kind == SymKind::Class {
            let internal = st.get(class_sym).jvm_name.clone();
            if internal.starts_with("scala/") {
                let full = internal.replace('/', ".");
                if let Some(m) = self.ensure_class(st, bin, &full, true) {
                    if m != class_sym {
                        out.extend(self.complete_on_class(st, bin, m, name));
                    }
                }
            }
        }
        // An *inherited* library member. `complete_named` reads a pickle only
        // for a library (or adopted) class, so a user class that extends one
        // gained nothing the prelude had not hand-written: `object Color
        // extends Enumeration` had no `values`, no `withName`, no `maxId`.
        // Ask the library ancestors themselves -- the member is installed on
        // the ancestor that declares it, which is the class the JVM call has
        // to name anyway, and ordinary member lookup finds it from there.
        //
        // Only when nothing else matched, so this stays additive.
        if out.is_empty() {
            out = self.complete_on_ancestors(st, bin, class_sym, name);
        }
        out
    }

    /// `cls`'s standard-library ancestors, nearest first, asked for the member
    /// one level at a time -- each level's *pickled* parents read before the
    /// walk steps past it.
    ///
    /// This used to take a snapshot of the parent lists already in the symbol
    /// table, and a stub's is empty until something reads its pickle, so a
    /// climb of more than one step stopped at the first. `MemberScope`'s bound
    /// leads to `Scopes$MemberScopeApi`, whose only pickled parent is
    /// `Scopes$ScopeApi`, whose only pickled parent is `Iterable[Symbol]`; the
    /// snapshot reached `ScopeApi`, `complete_on` attached `Iterable` to it a
    /// moment later, and nobody ever asked `Iterable`. `decls.collect` -- the
    /// first line of slick's `mapToImpl` -- was therefore "not a member of
    /// Scopes.MemberScope", and every `q"..."` after it inherited the error.
    ///
    /// Linearization order (parents last-first, breadth-first) so a member a
    /// subclass's own library parent declares wins over a grandparent's,
    /// exactly as `Check::enter_inherited_members` orders the scope. `cls`
    /// itself is not included: `complete_named` has already been asked about
    /// it.
    fn complete_on_ancestors(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        cls: SymbolId,
        name: &str,
    ) -> Vec<SymbolId> {
        let mut seen: Vec<u32> = vec![cls.0];
        let mut work: std::collections::VecDeque<SymbolId> =
            self.parents_after_pickle(st, bin, cls);
        while let Some(c) = work.pop_front() {
            if seen.contains(&c.0) || seen.len() > 256 {
                continue;
            }
            seen.push(c.0);
            let jvm = st.get(c).jvm_name.clone();
            // `scala/Any`, `scala/AnyRef` and friends have no pickle of their
            // own and would only cost a classfile miss per name.
            let library = jvm.starts_with("scala/") && jvm != "scala/Any" && jvm != "scala/AnyRef";
            // A `-cp` class outside `scala.*` is served by `complete_named`
            // only once it has been adopted, and nothing adopts a class the
            // program never names -- which is exactly an *inherited* one.
            // `class Users(t: Tag) extends Table[Int](t, "users")`, the shape
            // every slick testkit suite is written in, then had none of what
            // `Table` declares: `column`, `O` and `describe` were all "is not
            // a member", 695 diagnostics in one measurement, on a class whose
            // parent had resolved perfectly well.
            let adoptable = !library
                && !jvm.is_empty()
                && !jvm.starts_with("java/")
                && !jvm.starts_with("javax/")
                && self.adopt_binary_class(st, bin, c);
            if library || adoptable {
                let out = self.complete_on_class(st, bin, c, name);
                if !out.is_empty() {
                    return out;
                }
            }
            for p in self.parents_after_pickle(st, bin, c) {
                work.push_back(p);
            }
        }
        Vec::new()
    }

    /// A class's parents, after its own pickle has had the chance to add the
    /// ones the class file does not name. Reversed, for the linearization
    /// order `complete_on_ancestors` walks in.
    fn parents_after_pickle(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        cls: SymbolId,
    ) -> std::collections::VecDeque<SymbolId> {
        self.ensure_parents(st, bin, cls);
        st.get(cls)
            .parents
            .iter()
            .rev()
            .filter_map(|p| st.class_sym_of(p))
            .collect()
    }

    /// The `type` aliases a package object declares, read from its pickle.
    ///
    /// `po_full` is the package object's dotted name (`scala.package`).
    /// `Err` says the pickle is not there or does not parse -- the caller then
    /// supplies nothing, rather than guessing.
    pub fn package_object_aliases(
        &mut self,
        bin: &mut BinaryIndex,
        po_full: &str,
    ) -> Result<Vec<PickledAlias>, String> {
        // A package object can inherit exported aliases from a parent class:
        // cats' `package object data extends ScalaVersionSpecificPackage`
        // obtains `type NonEmptyLazyList[+A] = ...` this way.  `ClassSig` keeps
        // only direct members, so reading just `package$` loses the alias even
        // though its pickle is present on the parent.  Walk the same
        // linearization used by ordinary member completion and apply each
        // parent-to-child substitution before exposing the declaration.
        let mut src = BinSource(bin);
        self.sigs
            .class_sig(&mut src, po_full, true)
            .map_err(|e| e.to_string())?;
        let mut errs = Vec::new();
        let linearization = self.sigs.linearization(&mut src, po_full, true, &mut errs);
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for step in linearization {
            let Ok(sig) = self.sigs.class_sig(&mut src, &step.class_name, step.module) else {
                continue;
            };
            for m in &sig.members {
                if m.kind != MemberKind::TypeAlias
                    || !m.is_public_api()
                    || !seen.insert(m.name.clone())
                {
                    continue;
                }
                let ty = scala_rs_pickle::sym::apply_subst(&m.ty, &step.subst);
                let (tps, rhs) = match ty {
                    SigType::Poly { tparams, result } => (tparams, *result),
                    other => (Vec::new(), other),
                };
                out.push(PickledAlias {
                    name: scala_rs_pickle::names::decode_method_name(&m.name),
                    tparams: tps
                        .iter()
                        .map(|tp| PickledTParam {
                            name: tp.name.clone(),
                            arity: tparam_arity(tp),
                        })
                        .collect(),
                    rhs,
                });
            }
        }
        Ok(out)
    }

    /// Map a pickled type onto the typer's, in `scope`'s vocabulary.
    ///
    /// `None` means the type could not be expressed; the names that made it
    /// fail are then available from `take_unresolved_refs`, so the caller can
    /// load those classfiles and try again.
    pub fn convert_pickled_type(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        scope: &HashMap<String, Type>,
        t: &SigType,
    ) -> Option<Type> {
        self.unresolved_refs.clear();
        self.conv(st, bin, scope, t)
    }

    /// Dotted names the last `convert_pickled_type` could not resolve.
    pub fn take_unresolved_refs(&mut self) -> Vec<String> {
        std::mem::take(&mut self.unresolved_refs)
    }

    /// Complete variance and finality used by typed pattern compatibility.
    /// Provisional prelude flags must not make an abstract List look final.
    pub fn complete_class_pattern_metadata(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        class_sym: SymbolId,
    ) {
        if class_sym.is_none() || st.is_source_class(class_sym) {
            return;
        }
        let internal = st.get(class_sym).jvm_name.clone();
        let module = st.get(class_sym).kind == SymKind::ModuleClass;
        let Some(full) = self.pickled_full_name(bin, &internal, module) else {
            return;
        };
        let Ok(sig) = self.sigs.class_sig(&mut BinSource(bin), &full, module) else {
            return;
        };
        let flags = st.get(class_sym).flags;
        st.get_mut(class_sym).flags = Flags(
            (flags.0 & !Flags::FINAL.0)
                | if sig.flags & pflags::FINAL != 0 {
                    Flags::FINAL.0
                } else {
                    0
                },
        );
        let ids = st.get(class_sym).tparams.clone();
        if ids.len() != sig.tparams.len() {
            return;
        }
        for (id, tp) in ids.into_iter().zip(&sig.tparams) {
            st.get_mut(id).flags = st.get(id).flags.with(variance_flags(tp));
        }
    }

    /// Re-read a class just loaded from a `-cp` classfile from its own pickle.
    ///
    /// `install_java_class_in` builds the symbol out of the JVM *generic
    /// signature*, and that format cannot write a higher kind or a higher-kinded
    /// application. `trait Monad[F[_]]` arrives as `Monad[F]` with `F` a proper
    /// type, and `def pure[A](a: A): F[A]` as `(A)F` — so `Monad[F]` is a kind
    /// error at every use site and `F.pure(v)` is `found: F, required: F[R]`.
    /// scalac reads none of that from the signature: it reads the
    /// `ScalaSignature` pickle, which has the real one.
    ///
    /// So does this. The class keeps the symbol it already has (its parents,
    /// flags and fields all come from the classfile) and gains:
    ///
    /// * its pickled type parameters' *kinds*, and
    /// * one pickled signature per member the pickle declares, replacing the
    ///   JVM-signature member of the same name **only when the pickled one
    ///   could be expressed**. A member the pickle path declines is left
    ///   exactly as the classfile reader installed it, so this can add
    ///   precision but never take a member away.
    ///
    /// Only classes outside `scala.*` / `java.*`: the standard library reaches
    /// the typer through the hand-written prelude plus `complete`, and that
    /// path is the one the prelude's shapes are validated against.
    ///
    /// Returns true if the class was adopted (so `complete_named` may serve it
    /// from now on).
    pub fn adopt_binary_class(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        class_sym: SymbolId,
    ) -> bool {
        if class_sym.is_none() || !st.get(class_sym).is_class_like() {
            return false;
        }
        let internal = st.get(class_sym).jvm_name.clone();
        if internal.is_empty()
            || internal.starts_with("java/")
            || internal.starts_with("javax/")
            || internal.contains("$anon")
            // A *prelude* class is off limits: its signatures are hand-written
            // and the rest of the typer reasons about them, so rebuilding one
            // from a jar breaks members that work (the same reason
            // `ensure_class` refuses). The line is not `scala.*` though --
            // it is `scala.*` the prelude actually built. What it never names
            // (`scala.concurrent.Future`, `Promise`, `mutable.Growable` …)
            // had no source but the class file, where a by-name parameter is
            // an ordinary `Function0` and `Future(21)` does not typecheck.
            || (internal.starts_with("scala/") && class_sym.0 < st.prelude_end)
        {
            return false;
        }
        if self.adopted.contains(&class_sym.0) {
            return true;
        }
        let is_module = st.get(class_sym).kind == SymKind::ModuleClass;
        let Some(full) = self.pickled_full_name(bin, &internal, is_module) else {
            trace(format_args!("{internal}: no pickle to adopt"));
            return false;
        };
        let Ok(sig) = ({
            let mut src = BinSource(bin);
            self.sigs.class_sig(&mut src, &full, is_module)
        }) else {
            return false;
        };
        // Marked adopted before any member work: `complete_named` refuses a
        // class it does not know, and installing a member re-enters it (a
        // `$default$n` getter).
        self.adopted.insert(class_sym.0);
        trace(format_args!("adopting {full} (module={is_module})"));
        if is_nested_jvm_name(&internal) {
            if let Some((owner_name, _)) = full.rsplit_once('.') {
                if let Some(owner) =
                    self.ensure_class(st, bin, owner_name, sig.declaring_owner_is_module)
                {
                    st.binary_nested_decl_owners.insert(class_sym, owner);
                }
            }
        }
        adopt_tparam_kinds(st, class_sym, &sig);

        // Member names in declaration order, deduplicated: `complete_named`
        // installs every overload of a name at once.
        //
        // `MemberKind::Val` alongside `Def`: a package object's `val Resource
        // = cats.effect.kernel.Resource` compiles to a zero-arg accessor
        // method indistinguishable, at the class-file level, from an
        // ordinary `def` -- nothing in the bytecode says "this one is
        // stable". Excluding vals here left the raw-classfile `Method`
        // symbol `fill_java_members` had already installed as the only
        // description of `Resource`, and `ident_is_stable` (correctly)
        // refuses a bare `Method`. `Resource.ExitCase` then failed with
        // "stable identifier required, but Resource found" even though the
        // source is a plain `val`. `complete_named` installs a `Val` hit the
        // same way it installs a zero-clause `Def` and marks the result
        // `Flags::ACCESSOR`, which `ident_is_stable` / `member_is_stable` now
        // read as a val's stability, not a def's absence of it.
        let mut names: Vec<String> = Vec::new();
        for m in &sig.members {
            if m.kind != MemberKind::Def && m.kind != MemberKind::Val {
                continue;
            }
            // A case class's `apply` / `unapply` / `copy` are taken over too,
            // so the class file reader's description of them -- which cannot
            // say "implicit clause" or "this parameter has a default" -- is
            // dropped here rather than left to shadow the pickled one.
            if !m.is_inheritable_api()
                && !(m.has(pflags::PRIVATE) && !m.has(pflags::BRIDGE) && !m.has(pflags::SYNTHETIC))
                && !m.is_case_synthetic()
                && !m.is_case_copy(sig.flags)
                && !implicit_class_conversion_from(st, class_sym, &internal, m)
            {
                continue;
            }
            let src_name = scala_rs_pickle::names::decode_method_name(&m.name);
            if src_name.is_empty() || src_name == "<init>" || src_name.contains('$') {
                continue;
            }
            if !names.contains(&src_name) {
                names.push(src_name);
            }
        }
        // A companion module's classfile may carry an erased `apply` forwarder
        // for an inherited factory method even though the module's own pickle
        // has no `apply` member. Read that name through the pickle now, so the
        // erased forwarder cannot make an untyped varargs call enter the
        // tupled-application retry before the inherited method is visible.
        if is_module
            && !names.iter().any(|name| name == "apply")
            && st.get(class_sym).members.iter().any(|&member| {
                let symbol = st.get(member);
                symbol.kind == SymKind::Method && symbol.name == "apply"
            })
        {
            names.push("apply".to_string());
        }
        for name in names {
            // The eager, compact ScalaSignature reader installs value accessors
            // before this full pickle is available. It does not carry access
            // flags, and a private trait accessor has an expanded JVM name, so
            // descriptor completion below cannot replace that eager term.
            // Restore the declaration's private access on the surviving term:
            // a private value is not an inherited override candidate.
            if sig.members.iter().any(|member| {
                scala_rs_pickle::names::decode_method_name(&member.name) == name
                    && (member.kind == MemberKind::Val || member.has(pflags::STABLE))
                    && member.has(pflags::PRIVATE)
                    && !member.has_private_within
            }) {
                let eager_terms: Vec<_> = st
                    .get(class_sym)
                    .members
                    .iter()
                    .copied()
                    .filter(|&id| {
                        let member = st.get(id);
                        member.owner == class_sym
                            && member.name == name
                            && member.kind == SymKind::Term
                            && member.pickled_origin.is_empty()
                    })
                    .collect();
                for id in eager_terms {
                    st.get_mut(id).flags = st.get(id).flags.with(Flags::PRIVATE);
                }
            }
            // What the classfile reader put there, so it can be dropped once
            // the pickle has supplied something better.
            let stale: Vec<SymbolId> = st
                .get(class_sym)
                .members
                .iter()
                .copied()
                .filter(|&m| {
                    let s = st.get(m);
                    s.name == name
                        && (s.kind == SymKind::Method
                            // The eager classpath reader represents a Scala
                            // `val` accessor as a Term because the pickle
                            // subset marks the declaration as a value.  The
                            // full ScalaSignature later exposes that same
                            // accessor as a zero-argument Method.  Keep the
                            // prelude and source declarations intact, but
                            // remove this origin-less classpath Term when the
                            // richer pickled Method replaces it.
                            || (s.kind == SymKind::Term
                                && s.pickled_origin.is_empty()
                                && m.0 >= st.prelude_end
                                && m.0 < st.source_start))
                })
                .collect();
            let installed = self.complete_named(st, bin, class_sym, &name, false);
            if installed.is_empty() {
                continue;
            }
            // Scala 2's `Foo.class` static-forwarder view carries the full
            // pickle for the module, but the pickle itself describes the
            // source member as an instance method of `Foo$`. Preserve the
            // bytecode declaration's STATIC bit when replacing the stale
            // classfile member with that richer signature. Otherwise the
            // typer finds the right method but codegen emits
            // `Foo$.MODULE$.method`, even though the receiver selected the
            // forwarder as `Foo.method`.
            for &new_id in &installed {
                let jvm = st.get(new_id).jvm_name.clone();
                let forwarder = stale.iter().any(|&old_id| {
                    let old = st.get(old_id);
                    old.name == name && old.jvm_name == jvm && old.flags.contains(Flags::STATIC)
                });
                if forwarder {
                    st.get_mut(new_id).flags =
                        st.get(new_id).flags.with(Flags::STATIC).with(Flags::JAVA);
                }
            }
            drop_stale_members(st, class_sym, &stale, &installed);
        }
        self.drop_flattened_forwarders(st, bin, class_sym, &sig);
        self.drop_generic_mixin_forwarders(st, bin, class_sym, &sig);
        self.settle_overriding_type_aliases(st, bin, class_sym, &sig, &full, is_module);
        true
    }

    /// Drop generic JVM forwarders of methods inherited by a nested
    /// package-object module when its own Scala pickle is available.
    ///
    /// scala-rs emits an enclosing `ScalaSignature` on `package$child$`, while
    /// its classfile also contains concrete mixin forwarders. The latter only
    /// carry JVM generic signatures and can lose source precision: Cats'
    /// `option.none[A]: Option[A]` was read back as raw `Option`, and the
    /// direct forwarder then shadowed the precise `OptionSyntax#none` member.
    /// A method absent from the module's own pickle but present in a pickled
    /// parent is a forwarder, not a source override. Replace only generic
    /// methods of that exact self-output shape; top-level modules (including
    /// pos/t5639's monomorphic `Baz`) are untouched.
    fn drop_generic_mixin_forwarders(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        class_sym: SymbolId,
        sig: &scala_rs_pickle::sym::ClassSig,
    ) {
        let jvm = st.get(class_sym).jvm_name.clone();
        let nested_package_module = st.get(class_sym).kind == SymKind::ModuleClass
            && jvm
                .rsplit('/')
                .next()
                .is_some_and(|n| n.starts_with("package$") && n.ends_with('$'));
        if !nested_package_module {
            return;
        }
        let own: Vec<String> = sig
            .members
            .iter()
            .map(|m| scala_rs_pickle::names::decode_method_name(&m.name))
            .collect();
        let mut names = Vec::new();
        for &m in &st.get(class_sym).members {
            let s = st.get(m);
            if s.kind == SymKind::Method
                && !s.flags.contains(Flags::STATIC)
                && !s.name.contains('$')
                && !own.contains(&s.name)
                && !names.contains(&s.name)
            {
                names.push(s.name.clone());
            }
        }
        for name in names {
            let stale: Vec<SymbolId> = st
                .get(class_sym)
                .members
                .iter()
                .copied()
                .filter(|&m| {
                    let s = st.get(m);
                    s.kind == SymKind::Method && s.name == name && !s.flags.contains(Flags::STATIC)
                })
                .collect();
            let installed = self.complete_named(st, bin, class_sym, &name, false);
            if installed.is_empty() || !installed.iter().any(|&m| !st.get(m).tparams.is_empty()) {
                continue;
            }
            let arities: Vec<usize> = installed.iter().map(|&m| st.get(m).params.len()).collect();
            let stale: Vec<SymbolId> = stale
                .into_iter()
                .filter(|&m| arities.contains(&st.get(m).params.len()))
                .collect();
            if stale.is_empty() {
                continue;
            }
            trace(format_args!(
                "{jvm}#{name}: dropping {} generic mixin forwarder(s)",
                stale.len()
            ));
            drop_stale_members(st, class_sym, &stale, &installed);
        }
    }

    /// Drop a class file's *mixin forwarder* where the trait that really
    /// declares the method says it has more than one parameter clause.
    ///
    /// A class that mixes in a trait carries a forwarder for every concrete
    /// method it inherits, and that forwarder is an ordinary class-file
    /// method: one flat parameter list, and a `Signature` attribute in which
    /// a `Boolean` type *argument* has already become `Object`. The class's
    /// own pickle does not declare it -- it is inherited -- so the loop above
    /// never asks about the name, and the flattened copy stays as the class's
    /// closest member and wins every lookup over the correctly clause-split
    /// declaration on the trait.
    ///
    /// slick's `BaseColumnExtensionMethods[P1]` is the case. `ColumnExtension
    /// Methods` declares `def inSet[R](seq: Iterable[B1])(implicit om: …):
    /// Rep[R]`; the value class's own class file declares
    /// `inSet(Iterable<P1>, OptionMapper2<Object, P1, Object, Object, P1, R>)`,
    /// and `t.userName inSet Set("a")` was `no matching overload for
    /// (Iterable[String], OptionMapper2[Any, String, Any, Any, String, R])
    /// Rep[R] with arguments (Set[String])` -- one argument against a
    /// two-parameter clause. `===` escaped only because its JVM name is
    /// `$eq$eq$eq`, which [`fill_java_members`] never decodes, so the
    /// forwarder and the pickled member never shared a name.
    ///
    /// Deliberately narrow, in three ways, and each one was measured.
    ///
    /// * A name is only *considered* when the class file has a member of it
    ///   with **at least two parameters in one clause**. Splitting a parameter
    ///   list is the one thing a class file cannot express, and every split
    ///   form that matters here has two or more; a one-parameter
    ///   `()(implicit x)` is left alone rather than widening the walk over
    ///   every inherited unary method.
    /// * The replacement has to have **more than one clause**, and then
    ///   **every** class-file member of that name whose total parameter count
    ///   the replacement also has is dropped, not just the flattened one.
    ///   `DurationInt` inherits both `seconds: FiniteDuration` and
    ///   `seconds[C](c: C)(implicit ev: Classifier[C]): C#R`; dropping the
    ///   flattened two-parameter one and leaving the class file's nullary one
    ///   beside the pickle's left `2.seconds` an overload of three with a
    ///   duplicate in it, and `val f: FiniteDuration = 2.seconds` stopped
    ///   compiling (`crates/cli/tests/durrange.rs`, `setmap1`).
    /// * `complete_named` is asked of `class_sym`, not of the parent that
    ///   declares the name. At adoption time this class's parent list holds
    ///   only what its own pickle said (`AnyVal`, for a value class) --
    ///   the class file's parents are attached later -- so a walk over
    ///   `Symbol::parents` here reaches nothing, and `complete_named`'s own
    ///   walk over the *pickled* parents is what finds the declaration.
    fn drop_flattened_forwarders(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        class_sym: SymbolId,
        sig: &scala_rs_pickle::sym::ClassSig,
    ) {
        // Never the standard library, for the reason `ensure_pickled_parents`
        // and `attach_parents` already give: its hierarchy and its member sets
        // are the prelude's, hand-written and reasoned about, and topping one
        // up from a class file changes members that work. Measured:
        // `scala.collection.AbstractIterable` alone had eleven names this
        // would touch, and doing so gave `HashMap#toList` a second entry
        // (`<overload List[Tuple2[String, Any]] | List[(String, Any)]>`, and
        // `.sortBy` on it "not a member" -- `setmap1`) and `DurationInt`'s
        // `seconds` a third. What this is for is a *jar* class the program
        // named.
        let jvm = st.get(class_sym).jvm_name.clone();
        if jvm.starts_with("scala/") || jvm.starts_with("java/") {
            return;
        }
        let own: Vec<String> = sig
            .members
            .iter()
            .map(|m| scala_rs_pickle::names::decode_method_name(&m.name))
            .collect();
        let mut names: Vec<String> = Vec::new();
        for &m in &st.get(class_sym).members {
            let s = st.get(m);
            // Not gated on `Flags::JAVA`: a `-cp` **directory** is scanned by
            // `classpath::install_classpath` instead of read as a class file,
            // and that scan's entries carry no flag of their own -- but they
            // are just as flat, and just as erased (`(Iterable, Wit)Res`).
            // Everything reachable here is a binary class: `adopt_binary_class`
            // has already refused a source class and a prelude one.
            if s.kind != SymKind::Method
                || s.flags.contains(Flags::STATIC)
                || s.paramss.len() > 1
                || s.params.len() < 2
                || s.name.contains('$')
                || own.contains(&s.name)
                || names.contains(&s.name)
            {
                continue;
            }
            names.push(s.name.clone());
        }
        for name in names {
            let flat: Vec<(SymbolId, usize)> = st
                .get(class_sym)
                .members
                .iter()
                .copied()
                .filter(|&m| {
                    let s = st.get(m);
                    s.kind == SymKind::Method
                        && s.name == name
                        && !s.flags.contains(Flags::STATIC)
                        && s.paramss.len() <= 1
                })
                .map(|m| (m, st.get(m).params.len()))
                .collect();
            let installed = self.complete_named(st, bin, class_sym, &name, false);
            if !installed.iter().any(|&i| st.get(i).paramss.len() > 1) {
                continue;
            }
            let arities: Vec<usize> = installed
                .iter()
                .map(|&i| st.get(i).paramss.iter().map(|c| c.len()).sum())
                .collect();
            let stale: Vec<SymbolId> = flat
                .into_iter()
                .filter(|(m, n)| arities.contains(n) && !installed.contains(m))
                .map(|(m, _)| m)
                .collect();
            if stale.is_empty() {
                continue;
            }
            trace(format_args!(
                "{}#{name}: dropping {} flattened mixin forwarder(s)",
                st.get(class_sym).jvm_name,
                stale.len()
            ));
            drop_stale_members(st, class_sym, &stale, &installed);
        }
    }

    /// Install the type **aliases** this class declares that fix a type
    /// member an ancestor left deferred.
    ///
    /// An alias leaves no trace in the bytecode, so the class-file reader
    /// cannot know that `RelationalTableComponent.Table[T]` fixes
    /// `AbstractTable`'s `type TableElementType` to `T`. Until something asks
    /// for the name by hand, `SymbolTable::type_members_named` walks the
    /// parents and answers with the *abstract* declaration -- and every
    /// reduction of `E#TableElementType` at `E := Accounts` then has nothing
    /// to reduce to. This is `agent/backendtypes`' "a deferred declaration
    /// outranked the definition that fixes it", moved from lookup time to
    /// adoption time, because a reduction deep inside `subst_tparams` has no
    /// pickle to ask.
    ///
    /// Deliberately narrow: only an alias that *overrides* is installed.
    /// `slick.lifted.Aliases` declares thirty re-exports (`type Rep[T] =
    /// lifted.Rep[T]`) that override nothing, and installing those eagerly
    /// would change name resolution for every program that adopts the class
    /// without answering any question the symbol table got wrong.
    fn settle_overriding_type_aliases(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        class_sym: SymbolId,
        sig: &scala_rs_pickle::sym::ClassSig,
        full: &str,
        is_module: bool,
    ) {
        // Only a *nullary* alias. A parameterised one is a type constructor
        // whose expansion `expand_applied_hk_alias` drives at the use site,
        // and the on-demand path already installs it with its parameters.
        let aliases: Vec<(String, SigType)> = sig
            .members
            .iter()
            .filter(|m| {
                m.kind == MemberKind::TypeAlias
                    && m.is_public_api()
                    && !matches!(m.ty, SigType::Poly { .. })
            })
            .map(|m| (m.name.clone(), m.ty.clone()))
            .collect();
        if aliases.is_empty() {
            return;
        }
        let mut scope: HashMap<String, Type> = HashMap::new();
        for tp in &st.get(class_sym).tparams {
            scope.insert(st.get(*tp).name.clone(), Type::TypeParam(*tp));
        }
        for (name, rhs) in &aliases {
            // Already declared here: the class-file reader or an earlier
            // completion got there first, and this must not add a second.
            if st
                .type_members_named(class_sym, name)
                .into_iter()
                .any(|s| st.get(s).owner == class_sym)
            {
                continue;
            }
            let overrides = {
                let mut src = BinSource(bin);
                let (hits, _) = self.sigs.lookup(&mut src, full, is_module, name);
                hits.iter()
                    .any(|h| h.member.kind == MemberKind::AbstractType && h.owner != full)
            };
            if !overrides {
                continue;
            }
            let outer = self.self_ty.replace(Type::Class {
                sym: class_sym,
                args: Vec::new(),
            });
            let conv = self.conv_at(st, bin, &scope, rhs, 0);
            self.self_ty = outer;
            let Some(target) = conv else {
                trace(format_args!(
                    "{full}#{name}: overriding alias {rhs:?} does not convert"
                ));
                continue;
            };
            let id = st.alloc(name, class_sym, SymKind::TypeMember, Flags::EMPTY, "");
            st.get_mut(id).ty = target;
            st.get_mut(id).is_type_alias = true;
            st.get_mut(class_sym).members.push(id);
            // A prior lookup may have memoized the inherited abstract
            // declaration (or a less-specific alias) for this name.  The
            // concrete override changes the answer for every receiver that
            // can see this class, so none of those answers is reusable.
            self.invalidate_type_member_caches(name);
            trace(format_args!(
                "{full}#{name}: overriding type alias installed"
            ));
        }
    }

    /// Install the members `class_sym`'s pickle marks `implicit`, and only
    /// those.
    ///
    /// A companion object is in the implicit scope of its class (SLS 7.2), so
    /// a search for `Async[IO]` has to see `cats.effect.IO.asyncForIO`. Going
    /// through [`PickleSupply::adopt_binary_class`] to get it would install
    /// every one of `IO$`'s ~200 members, and completing each of those drags
    /// in most of cats-effect and cats-kernel — minutes of work for a
    /// six-line source file. The implicit scope needs the implicits; the rest
    /// of the companion is left to the ordinary on-demand path, which still
    /// serves it when the program actually names a member.
    ///
    /// Returns how many members were installed.
    pub fn supply_implicit_members(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        class_sym: SymbolId,
    ) -> usize {
        if class_sym.is_none() || !st.get(class_sym).is_class_like() {
            return 0;
        }
        // `scala.*` is not excluded. The only caller is
        // `Typer::load_companion_module`, which reaches here solely for a
        // companion object it has *just installed itself* -- a class the
        // prelude gave no companion at all -- so there is no hand-written
        // declaration here to protect. Without it a library companion the
        // prelude never names (`scala.collection.BuildFrom`) keeps the plain
        // classfile members the reader entered, which carry no `implicit`
        // flag, and its witnesses are in no implicit scope.
        let internal = st.get(class_sym).jvm_name.clone();
        if internal.is_empty() || internal.starts_with("java/") || internal.starts_with("javax/") {
            return 0;
        }
        if !self.implicits_supplied.insert(class_sym.0) {
            return 0;
        }
        let is_module = st.get(class_sym).kind == SymKind::ModuleClass;
        let Some(full) = self.pickled_full_name(bin, &internal, is_module) else {
            return 0;
        };
        let Ok(sig) = ({
            let mut src = BinSource(bin);
            self.sigs.class_sig(&mut src, &full, is_module)
        }) else {
            return 0;
        };
        let mut names: Vec<String> = Vec::new();
        for m in &sig.members {
            // `is_implicit_class_conversion`: an `implicit class`'s conversion
            // method is `SYNTHETIC`, which `is_public_api` hides.
            if !matches!(m.kind, MemberKind::Def | MemberKind::Module)
                || !m.has(pflags::IMPLICIT)
                || (!m.is_public_api()
                    && !implicit_class_conversion_from(st, class_sym, &internal, m))
            {
                continue;
            }
            let src_name = scala_rs_pickle::names::decode_method_name(&m.name);
            if src_name.is_empty() || src_name == "<init>" || src_name.contains('$') {
                continue;
            }
            if !names.contains(&src_name) {
                names.push(src_name);
            }
        }
        let mut n = 0;
        for name in names {
            // What the classfile reader put there, so it can be dropped once
            // the pickle has supplied something better -- two members of the
            // same name and arity would make every call ambiguous.
            let stale: Vec<SymbolId> = st
                .get(class_sym)
                .members
                .iter()
                .copied()
                .filter(|&m| {
                    let s = st.get(m);
                    s.name == name
                        && (s.kind == SymKind::Method
                            || (s.kind == SymKind::Term
                                && s.flags.contains(Flags::IMPLICIT)
                                && s.pickled_origin.is_empty()
                                && m.0 < st.source_start))
                })
                .collect();
            let installed = self.complete_named(st, bin, class_sym, &name, false);
            if installed.is_empty() {
                continue;
            }
            // Never drop a member `complete_named` itself reported back.
            // Completion caches the names it has already served, and when the
            // answer is a member the *pickle* installed earlier (an
            // `adopt_binary_class` of the same class), that member is both in
            // `stale` and in `installed`: removing it deleted the very
            // signature this call went to fetch, leaving the class with no
            // member of that name at all.
            drop_stale_members(st, class_sym, &stale, &installed);
            n += installed.len();
        }
        trace(format_args!("{full}: supplied {n} implicit member(s)"));
        n
    }

    /// Supply only the implicit declarations on `class_sym` whose result can
    /// provide `member_name` as an extension.
    ///
    /// A classpath class's Scala implicit members are not necessarily present
    /// in the shallow classpath scan.  This is especially visible for a
    /// `WordSpec`: `"name" when { ... }` is backed by the implicit conversion
    /// `convertToWordSpecStringWrapper`, whose declaration is in a pickled
    /// parent, while the JVM mixin forwarder has no implicit flag.  The normal
    /// extension search cannot discover that declaration until it has been
    /// installed on the inherited class symbol.
    ///
    /// The selected member name is not the conversion method name, so asking
    /// `complete_named` for `member_name` would be wrong.  Instead inspect the
    /// result class of each own implicit declaration and complete just the
    /// conversions whose result pickle declares the requested member.  When a
    /// result cannot be inspected, retain the declaration conservatively: the
    /// classfile may be the only description of that result type.
    pub(crate) fn supply_implicit_extensions_for(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        class_sym: SymbolId,
        member_name: &str,
    ) -> usize {
        if class_sym.is_none()
            || member_name.is_empty()
            || !st.get(class_sym).is_class_like()
            || !self.pickle_readable(st, class_sym)
        {
            return 0;
        }
        if !self
            .implicit_extensions_checked
            .insert((class_sym.0, member_name.to_string()))
        {
            return 0;
        }
        let Some(sig) = self.class_sig_of(st, bin, class_sym) else {
            return 0;
        };
        let encoded = scala_rs_pickle::names::encode_method_name(member_name);
        let internal = st.get(class_sym).jvm_name.clone();
        let mut names = Vec::new();
        for member in &sig.members {
            if !matches!(member.kind, MemberKind::Def | MemberKind::Module)
                || !member.has(pflags::IMPLICIT)
                || (!member.is_public_api()
                    && !implicit_class_conversion_from(st, class_sym, &internal, member))
            {
                continue;
            }
            let source_name = scala_rs_pickle::names::decode_method_name(&member.name);
            if source_name.is_empty() || source_name == "<init>" || source_name.contains('$') {
                continue;
            }
            let relevant = match sig_result_class_name(&member.ty) {
                Some(result) => self
                    .pickled_result_has_member(bin, result, &encoded)
                    .unwrap_or(true),
                None => true,
            };
            if relevant && !names.contains(&source_name) {
                names.push(source_name);
            }
        }
        // A Scala trait's concrete implicit conversion is emitted into the
        // implementing class as an ordinary one-argument JVM forwarder. It
        // is not in the class's own pickle, so `adopt_binary_class` cannot
        // replace it with a pickled declaration, and the forwarder would then
        // shadow the inherited implicit by name in `implicits_in_scope`.
        // Promote only a same-named Java forwarder when the parent pickle has
        // the relevant implicit conversion. This keeps the classfile method
        // (and its bytecode descriptor) while restoring the source-level
        // implicit flag; unrelated ordinary members remain untouched.
        let own_names: HashSet<String> = sig
            .members
            .iter()
            .map(|m| scala_rs_pickle::names::decode_method_name(&m.name))
            .collect();
        let mut parent_work: Vec<String> = sig
            .parents
            .iter()
            .filter_map(sig_parent_class_name)
            .collect();
        let mut parent_seen = HashSet::new();
        let mut promoted = 0;
        while let Some(parent) = parent_work.pop() {
            if !parent_seen.insert(parent.clone()) {
                continue;
            }
            let Ok(parent_sig) = ({
                let mut src = BinSource(bin);
                self.sigs.class_sig(&mut src, &parent, false)
            }) else {
                continue;
            };
            for member in &parent_sig.members {
                if !matches!(member.kind, MemberKind::Def | MemberKind::Module)
                    || !member.has(pflags::IMPLICIT)
                {
                    continue;
                }
                let source_name = scala_rs_pickle::names::decode_method_name(&member.name);
                if source_name.is_empty()
                    || source_name == "<init>"
                    || source_name.contains('$')
                    || own_names.contains(&source_name)
                {
                    continue;
                }
                let relevant = match sig_result_class_name(&member.ty) {
                    Some(result) => self
                        .pickled_result_has_member(bin, result, &encoded)
                        .unwrap_or(true),
                    None => true,
                };
                let forwarders: Vec<SymbolId> = st
                    .get(class_sym)
                    .members
                    .iter()
                    .copied()
                    .filter(|&id| {
                        let symbol = st.get(id);
                        symbol.owner == class_sym
                            && symbol.kind == SymKind::Method
                            && symbol.name == source_name
                            && symbol.flags.contains(Flags::JAVA)
                            && !symbol.flags.contains(Flags::IMPLICIT)
                    })
                    .collect();
                let has_forwarder = !forwarders.is_empty();
                for id in forwarders {
                    st.get_mut(id).flags = st.get(id).flags.with(Flags::IMPLICIT);
                    promoted += 1;
                }
                // A matching concrete forwarder already has the bytecode
                // descriptor needed by the receiver, so do not install a
                // second pickled copy on top of it. Without this guard the
                // direct forwarder and the inherited declaration become
                // duplicate candidates. If no forwarder exists, materialize
                // only conversions relevant to the selected extension name.
                // `enter_inherited_members` normally exposes the parent
                // declaration in the template scope already.  Materializing
                // a second copy on the child in that case would make
                // `shadow_inherited_implicits` treat the copy as a nearer
                // override and discard the actual parent candidate.  Only
                // supply a child member when the inherited declaration is not
                // otherwise visible (for example, a shallow classpath view
                // whose parent pickle was unavailable during scope setup).
                let scope_has_implicit = st.scopes.iter().rev().any(|scope| {
                    scope.lookup_ranked(&source_name).iter().any(|binding| {
                        let symbol = st.get(binding.sym);
                        symbol.flags.contains(Flags::IMPLICIT) && symbol.name == source_name
                    })
                });
                if !has_forwarder
                    && !scope_has_implicit
                    && relevant
                    && !names.contains(&source_name)
                {
                    names.push(source_name.clone());
                }
            }
            parent_work.extend(parent_sig.parents.iter().filter_map(sig_parent_class_name));
        }
        let mut supplied = 0;
        for name in names {
            supplied += self.complete_named(st, bin, class_sym, &name, false).len();
        }
        if supplied != 0 || promoted != 0 {
            trace(format_args!(
                "{}: supplied {supplied} implicit extension member(s) for {member_name} ({promoted} forwarder(s) promoted)",
                st.get(class_sym).jvm_name
            ));
        }
        supplied + promoted
    }

    /// The names of the implicit `def`s a class's own pickle declares.
    ///
    /// Members are read one name at a time, on demand, and an
    /// `import <a value>._` asks for *no* name in particular: it offers
    /// whatever the class has. `import seq.integral._` therefore brought no
    /// implicit into scope at all -- `Numeric#mkNumericOps` and
    /// `Ordering#mkOrderingOps` were never asked for, so `increment < zero`
    /// reported `value < is not a member of T`. This says which names the
    /// import has to ask for; the completion itself is the ordinary on-demand
    /// one, so a member the prelude already declares is untouched.
    ///
    /// Unlike [`Self::supply_implicit_members`] this is not restricted to
    /// classes outside `scala.*`: it installs nothing itself, and the caller
    /// only asks for a name the class has no member for.
    /// The names the library gives the parameters of `name` on `class_sym`,
    /// for the overload that takes `arity` value parameters in total.
    ///
    /// A pickled member arrives with its parameter *symbols*; a member the
    /// hand-written prelude declares carries only types, so a named argument
    /// on one had nothing to match against and reported "named arguments
    /// (method parameters not resolved)". This is the missing half, and only
    /// the names: the types stay the prelude's own, so nothing about how the
    /// call is typed or emitted changes.
    ///
    /// `None` unless exactly one set of names answers -- overloads of the same
    /// arity that disagree, a parameter nsc named `x$1` (a synthetic the
    /// source never wrote), and a class with no pickle all decline rather than
    /// guess.
    pub fn pickled_param_names(
        &mut self,
        bin: &mut BinaryIndex,
        internal: &str,
        is_module: bool,
        name: &str,
        arity: usize,
    ) -> Option<Vec<String>> {
        let full = self.pickled_full_name(bin, internal, is_module)?;
        let (hits, _errs) = {
            let mut src = BinSource(bin);
            self.sigs.lookup(&mut src, &full, is_module, name)
        };
        let mut found: Option<Vec<String>> = None;
        for h in &hits {
            let ps = sig_value_params(&h.member.ty);
            if ps.len() != arity {
                continue;
            }
            let names: Vec<String> = ps
                .iter()
                .map(|p| scala_rs_pickle::names::decode_method_name(&p.name))
                .collect();
            // nsc names a parameter the source did not `x$1`; matching an
            // argument against that would accept a name no programmer wrote.
            if names
                .iter()
                .any(|n| n.is_empty() || n.starts_with("x$") || n.starts_with("_$"))
            {
                return None;
            }
            match &found {
                None => found = Some(names),
                Some(prev) if *prev == names => {}
                Some(_) => return None,
            }
        }
        found
    }

    /// The names of the methods `class_sym`'s pickle declares **with a body**
    /// -- everything it defines rather than merely declares.
    ///
    /// Read-only on purpose. Completion is additive global state (see
    /// [`PickleSupply::complete`]'s own note about `BigDecimal`, where an
    /// unrelated completion changed which overload a later expression chose),
    /// and the one caller here only needs to know *whether* an override
    /// exists, not to have it. Installing it instead moved cats'
    /// `NonEmptyVector` from four diagnostics to five: asking `Vector` for
    /// `coll` / `toIterable` / `fromSpecific` -- all of which it does
    /// override, and none of which anything had asked for -- changed
    /// `toVector.grouped(n)` from `Seq` to `Iterable` and made `lazyZip`
    /// ambiguous.
    pub fn concrete_method_names(
        &mut self,
        st: &SymbolTable,
        bin: &mut BinaryIndex,
        class_sym: SymbolId,
    ) -> Vec<String> {
        if class_sym.is_none() || !st.get(class_sym).is_class_like() {
            return Vec::new();
        }
        if let Some(cached) = self.concrete_names.get(&class_sym.0) {
            return cached.clone();
        }
        let internal = st.get(class_sym).jvm_name.clone();
        let is_module = st.get(class_sym).kind == SymKind::ModuleClass;
        let mut names: Vec<String> = Vec::new();
        if !internal.is_empty() && !internal.starts_with("java/") && !internal.starts_with("javax/")
        {
            if let Some(full) = self.pickled_full_name(bin, &internal, is_module) {
                let sig = {
                    let mut src = BinSource(bin);
                    self.sigs.class_sig(&mut src, &full, is_module)
                };
                if let Ok(sig) = sig {
                    for m in &sig.members {
                        if m.kind != MemberKind::Def || m.has(pflags::DEFERRED) {
                            continue;
                        }
                        let src_name = scala_rs_pickle::names::decode_method_name(&m.name);
                        if src_name.is_empty() || src_name == "<init>" || src_name.contains('$') {
                            continue;
                        }
                        if !names.contains(&src_name) {
                            names.push(src_name);
                        }
                    }
                }
            }
        }
        self.concrete_names.insert(class_sym.0, names.clone());
        names
    }

    /// Names of source-level members declared directly by a binary class.
    /// Unlike `concrete_method_names`, this includes vals and nested types as
    /// well as methods, since all of them can be exposed by a wildcard import.
    pub(crate) fn pickled_member_names(
        &mut self,
        st: &SymbolTable,
        bin: &mut BinaryIndex,
        class_sym: SymbolId,
    ) -> Vec<String> {
        if class_sym.is_none() || !st.get(class_sym).is_class_like() {
            return Vec::new();
        }
        if let Some(cached) = self.pickled_member_names.get(&class_sym.0) {
            return cached.clone();
        }
        let internal = st.get(class_sym).jvm_name.clone();
        let is_module = st.get(class_sym).kind == SymKind::ModuleClass;
        let mut names = Vec::new();
        if !internal.is_empty() {
            if let Some(full) = self.pickled_full_name(bin, &internal, is_module) {
                let sig = {
                    let mut src = BinSource(bin);
                    self.sigs.class_sig(&mut src, &full, is_module)
                };
                if let Ok(sig) = sig {
                    for member in &sig.members {
                        let name = scala_rs_pickle::names::decode_method_name(&member.name);
                        // A val's private field is pickled as `answer ` (the
                        // local suffix); it is never a member an import sees.
                        if name.is_empty()
                            || name == "<init>"
                            || name.contains('$')
                            || name.ends_with(' ')
                        {
                            continue;
                        }
                        if !names.contains(&name) {
                            names.push(name);
                        }
                    }
                }
            }
        }
        self.pickled_member_names.insert(class_sym.0, names.clone());
        names
    }

    /// Materialize Scala default methods needed by a SAM anonymous class.
    ///
    /// A library class is normally completed one member name at a time.  That
    /// is the right trade-off for ordinary typing, but it is not enough for a
    /// generated SAM class: the JVM can resolve an inherited abstract method
    /// through a parent interface even when the child interface supplies a
    /// default, so the backend needs the child's complete default set to emit
    /// the same forwarding methods as scalac.  Keep this narrowly scoped to a
    /// type already being considered as a SAM; ordinary collection classes do
    /// not trigger eager completion.
    pub fn complete_sam_defaults(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        class_sym: SymbolId,
    ) {
        let mut work = vec![class_sym];
        let mut seen = rustc_hash::FxHashSet::default();
        while let Some(cls) = work.pop() {
            if cls.is_none() || !seen.insert(cls.0) {
                continue;
            }
            let names = self.concrete_method_names(st, bin, cls);
            for name in names {
                // A prelude/classfile scan may already have the concrete
                // declaration.  Re-reading that name through the pickle can
                // resolve an inherited declaration instead of the class's
                // own override (Ordering#equiv is the motivating shape),
                // replacing a known default with a deferred parent symbol.
                // Only ask the pickle for names that are not already
                // represented by a concrete member on this class.
                let already_concrete = st.get(cls).members.iter().any(|m| {
                    let member = st.get(*m);
                    member.kind == SymKind::Method
                        && member.name == name
                        && !st.method_is_deferred(*m)
                });
                if !already_concrete {
                    self.complete_on_class(st, bin, cls, &name);
                }
            }
            let parents = st.get(cls).parents.clone();
            for parent in parents {
                if let Some(p) = st.class_sym_of(&parent) {
                    work.push(p);
                }
            }
        }
    }

    pub fn implicit_member_names(
        &mut self,
        st: &SymbolTable,
        bin: &mut BinaryIndex,
        class_sym: SymbolId,
    ) -> Vec<String> {
        if class_sym.is_none() || !st.get(class_sym).is_class_like() {
            return Vec::new();
        }
        if let Some(cached) = self.implicit_names.get(&class_sym.0) {
            return cached.clone();
        }
        let internal = st.get(class_sym).jvm_name.clone();
        let is_module = st.get(class_sym).kind == SymKind::ModuleClass;
        let mut names: Vec<String> = Vec::new();
        if !internal.is_empty() && !internal.starts_with("java/") && !internal.starts_with("javax/")
        {
            if let Some(full) = self.pickled_full_name(bin, &internal, is_module) {
                let sig = {
                    let mut src = BinSource(bin);
                    self.sigs.class_sig(&mut src, &full, is_module)
                };
                if let Ok(sig) = sig {
                    for m in &sig.members {
                        // `is_implicit_class_conversion`: an `implicit class`'s
                        // conversion method is `SYNTHETIC`, which
                        // `is_public_api` hides.
                        if !matches!(m.kind, MemberKind::Def | MemberKind::Module)
                            || !m.has(pflags::IMPLICIT)
                            || (!m.is_public_api()
                                && !implicit_class_conversion_from(st, class_sym, &internal, m))
                        {
                            continue;
                        }
                        let src_name = scala_rs_pickle::names::decode_method_name(&m.name);
                        if src_name.is_empty() || src_name == "<init>" || src_name.contains('$') {
                            continue;
                        }
                        if !names.contains(&src_name) {
                            names.push(src_name);
                        }
                    }
                }
            }
        }
        self.implicit_names.insert(class_sym.0, names.clone());
        names
    }

    /// Whether this class declares a method with an explicit parameter clause
    /// followed by an implicit one.
    ///
    /// A classfile flattens those clauses into one ordinary JVM parameter
    /// list, so overload applicability cannot tell which trailing arguments
    /// Scala is allowed to synthesize. A wildcard import has no selected name
    /// to complete on demand; callers use this narrow predicate to adopt the
    /// module's pickle before entering its members.
    pub fn has_trailing_implicit_clause(
        &mut self,
        st: &SymbolTable,
        bin: &mut BinaryIndex,
        class_sym: SymbolId,
    ) -> bool {
        if class_sym.is_none() || !st.get(class_sym).is_class_like() {
            return false;
        }
        let internal = st.get(class_sym).jvm_name.clone();
        if internal.is_empty() || internal.starts_with("java/") || internal.starts_with("javax/") {
            return false;
        }
        let is_module = st.get(class_sym).kind == SymKind::ModuleClass;
        let Some(full) = self.pickled_full_name(bin, &internal, is_module) else {
            return false;
        };
        let sig = {
            let mut src = BinSource(bin);
            self.sigs.class_sig(&mut src, &full, is_module)
        };
        let Ok(sig) = sig else {
            return false;
        };
        sig.members.iter().any(|m| {
            m.kind == MemberKind::Def
                && m.is_public_api()
                && read_shape(&m.ty).is_some_and(|shape| {
                    shape.clauses.first().is_some_and(|c| !c.implicit)
                        && shape.clauses.iter().skip(1).any(|c| c.implicit)
                })
        })
    }

    /// The dotted name whose pickle describes `internal`, if there is one.
    ///
    /// A pickle names a nested class `Outer.Inner` while its classfile is
    /// `Outer$Inner`, so both spellings are tried (the `$` form first, since a
    /// `$` in a top-level name is part of that name).
    fn pickled_full_name(
        &mut self,
        bin: &mut BinaryIndex,
        internal: &str,
        is_module: bool,
    ) -> Option<String> {
        let full = internal.trim_end_matches('$').replace('/', ".");
        if self.has_pickle(bin, &full, is_module) {
            return Some(full);
        }
        let dotted = scala_rs_pickle::names::nested_to_dotted(&full);
        if dotted != full && self.has_pickle(bin, &dotted, is_module) {
            return Some(dotted);
        }
        None
    }

    /// Whether a class header has already been refined from its Scala pickle.
    pub(crate) fn parents_loaded(&self, class_sym: SymbolId) -> bool {
        self.parented.contains(&class_sym.0)
    }

    /// Give a binary class the constructors its pickle declares, when the
    /// symbol table has none of its own for it.
    ///
    /// nsc writes the `ScalaSignature` of a whole top-level class *once*, on
    /// the top-level class file; a nested class's own file carries no pickle
    /// at all (`javap -v q.Outer$Inner` shows a zero-length `Scala` marker,
    /// not a `ScalaSignature`). So a nested class reached through a type alias
    /// -- which is how every slick profile exports `Table`, `Query`,
    /// `Sequence` -- arrives with its members completed from the enclosing
    /// pickle by [`PickleSupply::adopt_binary_class`] and with **no `<init>`
    /// at all**, because that function skips `<init>` by name. Every
    /// `class As(tag: Tag) extends Table[Int](tag, "a")` was then "no matching
    /// overload for constructor Table", and with the parent in error the class
    /// body inherited nothing: `column`, `O` and `tableTag` were all "not
    /// found" behind it.
    ///
    /// Deliberately a *repair*, not an addition: a class that already has a
    /// constructor -- from source, from the prelude, or from its own class
    /// file's descriptors -- is left exactly as it is. Nothing that compiles
    /// today changes shape.
    ///
    /// The parameters installed are the pickle's, i.e. the **source**
    /// parameters, which is the same convention constructor symbols the typer
    /// builds itself follow; the backend prepends the hidden `$outer` at emit
    /// time (`gen::with_enclosing_outer_param`). The descriptor recorded is
    /// the real one out of the class file, so a call links.
    pub fn supply_ctors(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        class_sym: SymbolId,
    ) -> bool {
        if class_sym.is_none() || !st.get(class_sym).is_class_like() {
            return false;
        }
        // A constructor whose parameter list still holds an unresolved
        // `Type::Named` is not usable and not repairable: `parse_desc` reads a
        // descriptor with `&SymbolTable`, so a class the table had not heard
        // of yet when the descriptor was parsed stays a bare name that nothing
        // conforms to. `RelationalTableComponent$Table` arrived that way --
        // `(Tag, Option[String], String)Unit` with `Tag` unresolved -- and the
        // pickle, which is read with the table open, has the same declaration
        // with every name resolved. Those are replaced. Usable constructors
        // are still read from the pickle so its source-only parameter flags,
        // especially `DEFAULTPARAM`, can be merged into the descriptor-bearing
        // symbols already installed from the classfile.
        let broken: Vec<SymbolId> = st
            .get(class_sym)
            .members
            .iter()
            .copied()
            .filter(|&m| st.get(m).name == "<init>")
            .filter(|&m| ctor_has_unresolved_param(st, m))
            .collect();
        if !self.tried.insert((class_sym.0, "<init>".to_string())) {
            return false;
        }
        let internal = st.get(class_sym).jvm_name.clone();
        if internal.is_empty()
            || internal.starts_with("java/")
            || internal.starts_with("javax/")
            || internal.contains("$anon")
            || (internal.starts_with("scala/")
                && class_sym.0 < st.prelude_end
                && st
                    .get(class_sym)
                    .members
                    .iter()
                    .any(|m| st.get(*m).name == "<init>"))
        {
            return false;
        }
        let is_module = st.get(class_sym).kind == SymKind::ModuleClass;
        let Some(full) = self.pickled_full_name(bin, &internal, is_module) else {
            return false;
        };
        let Ok(sig) = ({
            let mut src = BinSource(bin);
            self.sigs.class_sig(&mut src, &full, is_module)
        }) else {
            return false;
        };
        let ctors: Vec<scala_rs_pickle::sym::Member> = sig
            .members
            .iter()
            // Deliberately **not** `is_public_api`, which hides a `private`
            // member outright. A `private` constructor has to be supplied and
            // *marked*, not dropped: dropped, `neg/t6601`'s
            // `new PrivateConstructor("")` in a separate compilation found no
            // constructor at all where nsc reports an access error -- and
            // before that, found the class file's `<init>` (emitted
            // `ACC_PUBLIC`, as nsc emits it) and accepted the call.
            .filter(|m| m.kind == MemberKind::Def && m.name == "<init>")
            .filter(|m| {
                !m.has(pflags::BRIDGE) && !m.has(pflags::SYNTHETIC) && !m.has(pflags::LOCAL)
            })
            .cloned()
            .collect();
        if ctors.is_empty() {
            return false;
        }
        // The class's own type parameters are the vocabulary a constructor's
        // parameters are written in.
        let mut scope: HashMap<String, Type> = HashMap::new();
        for tp in &st.get(class_sym).tparams {
            scope.insert(st.get(*tp).name.clone(), Type::TypeParam(*tp));
        }
        let saved_self = self.self_ty.replace(Type::Class {
            sym: class_sym,
            args: st
                .get(class_sym)
                .tparams
                .iter()
                .map(|t| Type::TypeParam(*t))
                .collect(),
        });
        let mut installed = 0usize;
        let mut seen: HashSet<String> = HashSet::new();
        for c in &ctors {
            if self.install_ctor(st, bin, class_sym, &internal, &scope, c, &mut seen, &broken) {
                installed += 1;
            }
        }
        self.self_ty = saved_self;
        if installed > 0 && !broken.is_empty() {
            st.get_mut(class_sym)
                .members
                .retain(|m| !broken.contains(m));
        }
        trace(format_args!(
            "{full}#<init>: supplied {installed} of {} pickled constructor(s), \
             replacing {} unreadable one(s)",
            ctors.len(),
            broken.len()
        ));
        installed > 0
    }

    /// One pickled `<init>`. Declines rather than guesses: a parameter type
    /// that does not convert, or a class file with no constructor whose
    /// parameters match, installs nothing.
    #[allow(clippy::too_many_arguments)]
    fn install_ctor(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        class_sym: SymbolId,
        internal: &str,
        class_scope: &HashMap<String, Type>,
        member: &scala_rs_pickle::sym::Member,
        seen: &mut HashSet<String>,
        broken: &[SymbolId],
    ) -> bool {
        let Some(shape) = read_shape(&member.ty) else {
            return false;
        };
        // The access the *pickle* records. The class file cannot carry it: nsc
        // emits even a `private` constructor `ACC_PUBLIC` (`javap -p` on
        // `class PrivateConstructor private(s: String) extends AnyVal` says
        // so), so this is the only place a separately compiled caller can
        // learn it.
        //
        // `private[p]` is pickled as `PRIVATE` **plus** a `privateWithin`
        // reference. The reader resolves `p` to its simple name, which is what
        // an access qualifier *is*: `access_within_of` walks out from the
        // member's own owner until it finds a class or package so named, so
        // two packages called `concurrent` cannot be confused. A reference
        // that did not resolve leaves the constructor accessible
        // (`ctor_access_flags`), because a boundary nothing can find denies
        // every call -- slick's own `private[slick]` constructors included.
        let access = ctor_access_flags(member);
        let within = ctor_access_within(member);
        // A constructor takes no type parameters of its own; nsc writes the
        // class's as a `POLYtpe` wrapper, and those are already in scope.
        // `CONSTRUCTOR`, not merely the name: it is what
        // `access_full_location_string` reads to call this a *constructor*,
        // and nsc's diagnostic is "constructor Qual in class Qual cannot be
        // accessed", never "method <init>".
        let m = st.alloc(
            "<init>",
            SymbolId::NONE,
            SymKind::Method,
            access.with(Flags::CONSTRUCTOR),
            "",
        );
        // Independent of the flag, and that is the whole shape of it: nsc
        // pickles `private[p]` as a `privateWithin` reference with **no**
        // `PRIVATE` flag at all (`javap`-visible proof is impossible, but the
        // pickle for `class Qual private[libp] (...)` compiled by scalac
        // 2.13.16 reads `flags=0x200 private=false privateWithin=libp`).
        // `Typer::accessible` treats a bare `private_within` as restricted for
        // exactly that reason. `protected[p]` does carry `PROTECTED`, and
        // takes the qualifier through the same field.
        if within.is_some() {
            st.get_mut(m).private_within = within.clone();
        }
        let mut paramss_ty: Vec<Vec<Type>> = Vec::new();
        let mut paramss_sym: Vec<Vec<SymbolId>> = Vec::new();
        for clause in &shape.clauses {
            let mut tys = Vec::new();
            let mut syms = Vec::new();
            for p in &clause.params {
                let Some(mut t) = self.conv(st, bin, class_scope, &p.ty) else {
                    trace(format_args!(
                        "{internal}#<init>: parameter {} has an unmappable type {:?}",
                        p.name, p.ty
                    ));
                    return false;
                };
                if p.by_name && !matches!(t, Type::ByName(_)) {
                    t = Type::ByName(Box::new(t));
                }
                let mut flags = if clause.implicit {
                    Flags::PARAM.with(Flags::IMPLICIT)
                } else {
                    Flags::PARAM
                };
                // The classfile has no parameter-level default bit. Keep the
                // source pickle's flag on the constructor symbol so the
                // caller can resolve the corresponding JVM getter later.
                if p.flags & pflags::DEFAULTPARAM != 0 {
                    flags = flags.with(Flags::DEFAULTPARAM);
                }
                let ps = st.alloc(
                    scala_rs_pickle::names::decode_method_name(&p.name),
                    m,
                    SymKind::Term,
                    flags,
                    "",
                );
                st.get_mut(ps).ty = t.clone();
                tys.push(t);
                syms.push(ps);
            }
            paramss_ty.push(tys);
            paramss_sym.push(syms);
        }
        let want: Vec<Option<String>> = paramss_ty
            .iter()
            .flatten()
            .map(|t| erased_param_desc(st, t))
            .collect();
        let key = format!("{want:?}");
        if !seen.insert(key) {
            return false;
        }
        let Some(desc) = self.ctor_desc(st, bin, class_sym, internal, &want) else {
            trace(format_args!(
                "{internal}#<init>: no constructor in the class file matches {want:?}"
            ));
            return false;
        };
        // Constructor applications are flattened by the typer (`new C(a)(b)`
        // and `extends C(a)(b)` both reach overload selection as one argument
        // list). Keep that same view on the installed symbol while retaining
        // each source parameter's flags, including DEFAULTPARAM. The default
        // getter index is global across clauses. Keep the symbol clauses
        // separately so named/default placement respects source boundaries.
        let source_params: Vec<SymbolId> = paramss_sym.iter().flatten().copied().collect();
        let source_param_types: Vec<Type> = paramss_ty.iter().flatten().cloned().collect();
        let source_paramss = paramss_sym;
        let hidden_outer = self
            .java_class(bin, internal)
            .and_then(|c| hidden_outer_desc(st, class_sym, c));
        st.get_mut(class_sym).binary_outer_desc = hidden_outer.clone();
        let existing = st.get(class_sym).members.iter().copied().find(|&id| {
            if broken.contains(&id) {
                return false;
            }
            let s = st.get(id);
            if s.name != "<init>" || s.owner != class_sym {
                return false;
            }
            s.jvm_name == desc
                || (s.jvm_name.is_empty()
                    && ctor_params_match(st, id, &want, hidden_outer.as_deref()))
        });
        if let Some(existing) = existing {
            // Keep the JVM descriptor already read from the classfile, but
            // replace its erased parameter view with the source-shaped one.
            // An inner class may have one hidden leading outer parameter in
            // the descriptor; it is supplied by codegen, not by Scala source.
            let old_params = st.get(existing).params.clone();
            st.get_mut(existing)
                .members
                .retain(|id| !old_params.contains(id));
            st.get_mut(m).members.clear();
            st.get_mut(m).params.clear();
            st.get_mut(m).paramss.clear();
            st.get_mut(m).ty = Type::NoType;
            for &param in &source_params {
                st.get_mut(param).owner = existing;
                if !st.get(existing).members.contains(&param) {
                    st.get_mut(existing).members.push(param);
                }
            }
            st.set_jvm_name(existing, desc);
            // The symbol being repaired came from the class file's descriptor,
            // where the constructor is `ACC_PUBLIC` whatever the source said.
            // Only ever *adds* the pickle's access: nothing here can widen a
            // constructor the typer already knows to be restricted.
            if access != Flags::EMPTY {
                let f = st.get(existing).flags.with(access);
                st.get_mut(existing).flags = f;
            }
            // The boundary travels whether or not a flag came with it, or the
            // pair means the wrong thing: `PRIVATE` alone is `private`, a
            // qualifier alone is `private[p]`, and `PROTECTED` with a
            // qualifier is `protected[p]`. Dropping the qualifier at the
            // moment the descriptor is repaired would turn every
            // `private[slick]` constructor into a plain public one again.
            if within.is_some() {
                st.get_mut(existing).private_within = within.clone();
            }
            st.get_mut(existing).params = source_params.clone();
            st.get_mut(existing).paramss = source_paramss.clone();
            st.get_mut(existing).ty = Type::Method {
                paramss: vec![source_param_types.clone()],
                ret: Box::new(Type::Unit),
            };
            return true;
        }
        st.set_jvm_name(m, desc.clone());
        st.get_mut(m).params = source_params;
        st.get_mut(m).paramss = source_paramss;
        st.get_mut(m).ty = Type::Method {
            paramss: vec![source_param_types],
            ret: Box::new(Type::Unit),
        };
        st.get_mut(m).owner = class_sym;
        st.get_mut(class_sym).members.push(m);
        true
    }

    /// The real `<init>` descriptor of `internal` whose *trailing* parameters
    /// are `want`.
    ///
    /// Only the class's own file is searched -- constructors are not
    /// inherited. A Scala inner class's constructor carries the enclosing
    /// instance ahead of the source parameters, so one extra leading
    /// parameter is allowed; more than one candidate is ambiguous and
    /// declines.
    fn ctor_desc(
        &mut self,
        st: &SymbolTable,
        bin: &mut BinaryIndex,
        class_sym: SymbolId,
        internal: &str,
        want: &[Option<String>],
    ) -> Option<String> {
        let jc = self.java_class(bin, internal)?;
        let hidden_outer = hidden_outer_desc(st, class_sym, jc);
        let mut exact_hits: Vec<String> = Vec::new();
        let mut hidden_hits: Vec<String> = Vec::new();
        for jm in &jc.methods {
            if jm.name != "<init>" || jm.access & (ACC_BRIDGE | ACC_SYNTHETIC | ACC_STATIC) != 0 {
                continue;
            }
            let Some(got) = desc_params(&jm.desc) else {
                continue;
            };
            let exact = got.len() == want.len();
            let hidden = hidden_outer.as_deref().is_some_and(|outer| {
                got.len() == want.len() + 1 && got.first().is_some_and(|p| p == outer)
            });
            if !exact && !hidden {
                continue;
            }
            let tail = &got[got.len() - want.len()..];
            let ok = tail.iter().zip(want).all(|(g, w)| match w {
                Some(w) => g == w,
                None => g.starts_with('L') || g.starts_with('['),
            });
            if ok {
                let hits = if exact {
                    &mut exact_hits
                } else {
                    &mut hidden_hits
                };
                if !hits.contains(&jm.desc) {
                    hits.push(jm.desc.clone());
                }
            }
        }
        // A non-inner class can have a legitimate overload whose source
        // parameters happen to equal the tail of another descriptor. It must
        // use the exact descriptor; accepting a tail by arity would silently
        // bind source metadata to the wrong constructor. A non-static nested
        // class always has the hidden outer slot, so only its tail candidates
        // are eligible.
        let hits = if hidden_outer.is_some() {
            hidden_hits
        } else {
            exact_hits
        };
        if hits.len() == 1 {
            hits.into_iter().next()
        } else {
            None
        }
    }

    /// The cache is keyed by receiver and name, but an alias installed on an
    /// ancestor changes the answer for all of its descendants.  Clearing all
    /// entries for the name is deliberately conservative: the table is
    /// small compared with the pickle work being avoided, and it also covers
    /// receivers that were completed before their inheritance links were
    /// fully materialized.
    fn invalidate_type_member_caches(&mut self, name: &str) {
        self.tried_types
            .retain(|(_, cached_name), _| cached_name != name);
        self.completed_type_member_decls
            .retain(|(_, cached_name), _| cached_name != name);
    }

    /// Install the **type** member `name` of `class_sym`, read from the pickle
    /// of whichever class in its linearisation declares it.
    ///
    /// [`PickleSupply::complete`] is the term namespace; this is the type one,
    /// which the reflection API is written almost entirely in.
    /// `scala.reflect.macros.blackbox.Context` inherits
    ///
    /// ```text
    /// type Tree      = universe.Tree            // from scala.reflect.macros.Aliases
    /// type Expr[T]   = universe.Expr[T]
    /// type WeakTypeTag[T] = universe.WeakTypeTag[T]
    /// ```
    ///
    /// and a macro implementation cannot state its own signature without them
    /// (`docs/macros.md` §7.6). Abstract members (`type Tree >: Null <:
    /// TreeApi`) go to [`PickleSupply::abstract_type_member`], which keeps
    /// their bounds; aliases are installed with their right-hand side, so
    /// `c.Expr[Int]` really is `scala.reflect.api.Exprs.Expr[Int]`.
    ///
    /// **The prefix is dropped.** `universe.Expr[T]` becomes the class
    /// `Exprs.Expr[T]`, not that class *as seen from* this particular `c`, so
    /// two macro implementations' `Expr`s are one type here and are two in
    /// nsc. That is a loss of precision inside a macro implementation's own
    /// body, never a difference in what is emitted: the erased signature the
    /// backend writes is `scala/reflect/api/Exprs$Expr` either way, which is
    /// exactly what `Context.prefix()` returns in the classfile.
    ///
    /// `None` when nothing declares it, or when the right-hand side cannot be
    /// expressed -- an alias installed with no target would silently be an
    /// opaque type that conforms to nothing.
    pub fn complete_type_member(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        class_sym: SymbolId,
        name: &str,
    ) -> Option<Type> {
        if class_sym.is_none() || name.is_empty() {
            return None;
        }
        // An answer already in the table is not authoritative here. A
        // deferred member reached through a parent may be shadowed by a
        // concrete alias in the receiver's subclass, and even a concrete
        // declaration may have been installed from a less-specific lookup.
        // Resolve the pickle's linearisation first so the declaration origin
        // and its concrete RHS stay together; use the table only as a final
        // fallback when no pickle can answer.
        let key = (class_sym.0, name.to_string());
        if let Some(memo) = self.tried_types.get(&key) {
            return memo.clone();
        }
        // The declaration cache is paired with `tried_types`, but a previous
        // attempt may have left only the declaration behind (for example when
        // an alias RHS failed to convert). Never let that orphaned symbol be
        // used for a newly resolved answer.
        self.completed_type_member_decls.remove(&key);
        let outer_for = self.completing_for.replace(class_sym);
        let resolution = self.complete_type_member_uncached(st, bin, class_sym, name);
        self.completing_for = outer_for;
        let mut answer = resolution.as_ref().map(|r| r.ty.clone());
        if let Some(decl) = resolution.and_then(|r| r.decl) {
            self.completed_type_member_decls.insert(key.clone(), decl);
        }
        // No pickle said anything better, so the inherited declaration stands.
        if answer.is_none() {
            let installed = st
                .type_members_named(class_sym, name)
                .into_iter()
                .find(|&s| st.get(s).kind == SymKind::TypeMember);
            answer = installed.map(|id| {
                self.completed_type_member_decls.insert(key.clone(), id);
                st.type_member_as_seen(id)
            });
        }
        self.tried_types.insert(key, answer.clone());
        answer
    }

    /// Expand a private alias named by a method declared in the same binary
    /// class. The method's public signature may use the alias even though a
    /// caller cannot select it by name. Read its RHS without installing the
    /// private declaration as a visible member of the receiving class.
    pub(crate) fn expand_declared_method_alias(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        owner: SymbolId,
        name: &str,
        args: &[Type],
    ) -> Option<Type> {
        let symbol = st.get(owner);
        if !symbol.is_class_like() {
            return None;
        }
        let internal = symbol.jvm_name.clone();
        let is_module = symbol.kind == SymKind::ModuleClass;
        let full = self.pickled_full_name(bin, &internal, is_module)?;
        let sig = self
            .sigs
            .class_sig(&mut BinSource(bin), &full, is_module)
            .ok()?;
        let alias = sig
            .members
            .iter()
            .find(|member| member.name == name && member.kind == MemberKind::TypeAlias)?;
        let (tparams, rhs) = match &alias.ty {
            SigType::Poly { tparams, result } => (tparams.as_slice(), result.as_ref()),
            other => (&[][..], other),
        };
        if tparams.len() != args.len() {
            return None;
        }
        let owner_tparams = st.get(owner).tparams.clone();
        let mut scope: HashMap<String, Type> = owner_tparams
            .iter()
            .map(|&id| (st.get(id).name.clone(), Type::TypeParam(id)))
            .collect();
        scope.extend(
            tparams
                .iter()
                .zip(args)
                .map(|(param, ty)| (param.name.clone(), ty.clone())),
        );
        let outer = self.self_ty.replace(Type::Class {
            sym: owner,
            args: owner_tparams.into_iter().map(Type::TypeParam).collect(),
        });
        let expanded = self.conv_at(st, bin, &scope, rhs, 0);
        self.self_ty = outer;
        expanded
    }

    /// Return the declaration selected by [`complete_type_member`], including
    /// the synthetic declaration retained for a transparent alias.
    pub(crate) fn completed_type_member_decl(
        &self,
        class_sym: SymbolId,
        name: &str,
    ) -> Option<SymbolId> {
        self.completed_type_member_decls
            .get(&(class_sym.0, name.to_string()))
            .copied()
    }

    fn complete_type_member_uncached(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        class_sym: SymbolId,
        name: &str,
    ) -> Option<CompletedTypeMember> {
        let sym = st.get(class_sym);
        if !sym.is_class_like() {
            return None;
        }
        let internal = sym.jvm_name.clone();
        // `complete_named`'s gate does *not* apply here, and copying it was a
        // bug. A term member has a classfile fallback: decline it and the
        // method the classfile reader installed still describes it. A `type`
        // member has none -- an alias leaves no trace in the bytecode at all,
        // it exists only in the `ScalaSignature` pickle. Declining one is not
        // "less precision", it is the name not existing.
        //
        // That is what `import profile.api.*` ran into across slick's testkit:
        // `slick.lifted.Aliases` declares `type Rep[T] = lifted.Rep[T]`,
        // `type Table[T]`, `type DBIO[+R]` and thirty more, and none of them
        // resolved unless something else had happened to adopt the class
        // first. 1141 of 2112 errors in one measurement.
        //
        // What is excluded is only what cannot have one: Java classes carry no
        // pickle. `scala/*` was already served here unconditionally, prelude
        // or not, and stays that way.
        if internal.is_empty()
            || internal.starts_with("java/")
            || internal.starts_with("javax/")
            || internal.contains("$anon")
        {
            trace(format_args!(
                "{internal}#{name}: no pickle to read a type from"
            ));
            return None;
        }
        let is_module = sym.kind == SymKind::ModuleClass;
        let full = self
            .pickled_full_name(bin, &internal, is_module)
            .unwrap_or_else(|| internal.trim_end_matches('$').replace('/', "."));
        let (hits, _errs) = {
            let mut src = BinSource(bin);
            self.sigs.lookup(&mut src, &full, is_module, name)
        };
        trace(format_args!("{full}#{name}: {} pickle hit(s)", hits.len()));
        for h in &hits {
            trace(format_args!(
                "  hit {}#{name} kind={:?} public={} ty={:?}",
                h.owner,
                h.member.kind,
                h.member.is_public_api(),
                h.member.ty
            ));
        }
        let type_hit = hits
            .iter()
            .find(|h| {
                matches!(
                    h.member.kind,
                    MemberKind::TypeAlias | MemberKind::AbstractType
                ) && h.member.is_public_api()
            })
            .cloned();
        if let Some(hit) = type_hit {
            if hit.member.kind == MemberKind::AbstractType {
                let qualified = format!("{}.{name}", hit.owner);
                let ty = self.abstract_type_member(st, bin, &qualified, 0)?;
                let decl = match &ty {
                    Type::TypeMember(id) => Some(*id),
                    _ => None,
                };
                return Some(CompletedTypeMember { ty, decl });
            }
            // A module's aliases belong to its module class. Looking up
            // the same name as a non-module can fail (Predef) or attach the
            // alias to an unrelated companion class.
            // An inherited alias needs a declaration in the receiver's
            // vocabulary. Installing it on `hit.owner` would either leave its
            // owner parameters unbound at use sites or, if the substituted
            // hit RHS were used, specialize that shared declaration to the
            // first concrete subclass that asked for it.
            let alias_owner = class_sym;
            let declaring_owner = if hit.owner != full || hit.owner_module != is_module {
                self.ensure_class(st, bin, &hit.owner, hit.owner_module)
            } else {
                None
            };
            // The RHS is installed on `class_sym`, so use the substituted
            // member from `lookup`. Its alias prefix still belongs to the
            // declaring member and is needed to retain path-dependent inner
            // class prefixes.
            let declared_member = if hit.owner == full && hit.owner_module == is_module {
                hit.member.clone()
            } else {
                let sig = {
                    let mut src = BinSource(bin);
                    self.sigs
                        .class_sig(&mut src, &hit.owner, hit.owner_module)
                        .ok()?
                };
                sig.members
                    .iter()
                    .find(|member| {
                        member.name == hit.member.name && member.kind == MemberKind::TypeAlias
                    })?
                    .clone()
            };
            if let Some(prefix) = &declared_member.alias_prefix {
                trace(format_args!(
                    "{}#{name}: alias prefix {prefix:?}",
                    hit.owner
                ));
                st.binary_alias_prefixes
                    .insert((alias_owner, name.to_string()), prefix.clone());
            }
            let this_prefix = match &declared_member.alias_prefix {
                Some(SigType::This(c)) => Some(c.clone()),
                _ => None,
            };
            let ty = self.install_type_alias(
                st,
                bin,
                alias_owner,
                name,
                &hit.member.ty,
                this_prefix.as_deref(),
            )?;
            // `install_type_alias` intentionally returns a transparent
            // nullary alias as its RHS. Retain an owner-local declaration for
            // dependent paths, without changing the transparent type seen by
            // ordinary callers.
            let decl = match &ty {
                Type::TypeMember(id) => Some(*id),
                _ => {
                    let id = st.alloc(name, alias_owner, SymKind::TypeMember, Flags::EMPTY, "");
                    st.get_mut(id).ty = ty.clone();
                    st.get_mut(id).is_type_alias = true;
                    st.get_mut(alias_owner).members.push(id);
                    self.invalidate_type_member_caches(name);
                    Some(id)
                }
            };
            if let (Some(id), Some(declaring_owner)) = (decl, declaring_owner) {
                if st.get(id).owner == alias_owner && st.get(id).name == name {
                    st.binary_alias_decl_owners.insert(id, declaring_owner);
                }
            }
            return Some(CompletedTypeMember { ty, decl });
        }
        // A *nested class or trait* named as a **type**, as opposed to a type
        // alias or an abstract type member: `u.TypeTag[T]` (`TypeTags.TypeTag`),
        // `c.universe.Transformer` (`Trees.Transformer`), `u.Liftable[Int]`
        // (`Liftables.Liftable`). The reflection API is written almost
        // entirely of these. `complete_named`'s `MemberKind::Module` arm
        // supplies the *term* half of the same shape (a nested `object`, via
        // `install_nested_module`); nothing supplied the *type* half before
        // this, so naming the class itself gave "not a member of Universe" /
        // "not found: type ..." even though the classfile genuinely exists on
        // the classpath.
        let class_hit = hits
            .into_iter()
            .find(|h| h.member.kind == MemberKind::Class && h.member.is_public_api())?;
        let qualified = format!("{}.{name}", class_hit.owner);
        let sym = self.ensure_class(st, bin, &qualified, false)?;
        Some(CompletedTypeMember {
            ty: Type::Class {
                sym,
                args: Vec::new(),
            },
            decl: None,
        })
    }

    /// A type member that `class_sym`, a class read from a jar, passes on to a
    /// subclass: what a bare `Reader` means inside a source class that
    /// extends it.
    ///
    /// [`PickleSupply::complete_type_member`] answers a *selection* -- `p.T`,
    /// `C#T`, a name an import offers -- and so only a public member. A
    /// subclass body sees more than that: slick's `ResultConverter` declares
    /// `protected[this] type Reader = M#Reader`, and every `mapTo` expansion
    /// overrides `read(r: Reader)` in an anonymous subclass of
    /// `SimpleFastPathResultConverter`. Nothing in the bytecode records an
    /// alias, and the protected one was never installed, so the name was not
    /// found at all.
    ///
    /// The member is installed as a symbol of the class that **declares** it,
    /// in that class's own vocabulary (the pickle is re-read there rather than
    /// through `class_sym`, whose lookup substitutes into `class_sym`'s type
    /// parameters). Seen from the subclass, the typer then substitutes the
    /// subclass's base-type arguments for the declaring class's parameters,
    /// exactly as it does for a source ancestor's alias.
    ///
    /// A `private` member is not inherited (SLS 5.2) and is not offered; a
    /// `protected` or `protected[this]` one is, and keeps `PROTECTED`.
    pub fn complete_inherited_type_member(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        class_sym: SymbolId,
        name: &str,
    ) -> Option<SymbolId> {
        if class_sym.is_none() || name.is_empty() || !st.get(class_sym).is_class_like() {
            return None;
        }
        let internal = st.get(class_sym).jvm_name.clone();
        if internal.is_empty()
            || internal.starts_with("java/")
            || internal.starts_with("javax/")
            || internal.contains("$anon")
        {
            return None;
        }
        let is_module = st.get(class_sym).kind == SymKind::ModuleClass;
        let full = self.pickled_full_name(bin, &internal, is_module)?;
        let inheritable = |m: &scala_rs_pickle::sym::Member| {
            matches!(m.kind, MemberKind::TypeAlias | MemberKind::AbstractType)
                && !m.has(pflags::PRIVATE)
                && !m.has(pflags::SYNTHETIC)
                && !m.has(pflags::BRIDGE)
        };
        let hit = {
            let mut src = BinSource(bin);
            let (hits, _) = self.sigs.lookup(&mut src, &full, is_module, name);
            hits.into_iter().find(|h| inheritable(&h.member))?
        };
        let owner = if hit.owner == full && hit.owner_module == is_module {
            class_sym
        } else {
            self.ensure_class(st, bin, &hit.owner, hit.owner_module)?
        };
        if let Some(id) = st
            .get(owner)
            .members
            .iter()
            .copied()
            .find(|&m| st.get(m).kind == SymKind::TypeMember && st.get(m).name == name)
        {
            return Some(id);
        }
        // Re-read in the declaring class's own vocabulary.
        let declared = {
            let mut src = BinSource(bin);
            let sig = self
                .sigs
                .class_sig(&mut src, &hit.owner, hit.owner_module)
                .ok()?;
            sig.members
                .iter()
                .find(|m| m.name == name && inheritable(m))?
                .clone()
        };
        let id = if declared.kind == MemberKind::AbstractType {
            match self.abstract_type_member(st, bin, &format!("{}.{name}", hit.owner), 0)? {
                Type::TypeMember(id) => id,
                _ => return None,
            }
        } else if matches!(declared.ty, SigType::Poly { .. }) {
            let this_prefix = match &declared.alias_prefix {
                Some(SigType::This(c)) => Some(c.clone()),
                _ => None,
            };
            match self.install_type_alias(
                st,
                bin,
                owner,
                name,
                &declared.ty,
                this_prefix.as_deref(),
            )? {
                Type::TypeMember(id) => id,
                _ => return None,
            }
        } else {
            // A nullary alias. `install_type_alias` hands one back as its
            // right-hand side with no symbol; a subclass needs the symbol, so
            // that the declaring class's parameters in the right-hand side can
            // be seen through the subclass's base type.
            let mut scope: HashMap<String, Type> = HashMap::new();
            for tp in &st.get(owner).tparams {
                scope.insert(st.get(*tp).name.clone(), Type::TypeParam(*tp));
            }
            let outer = self.self_ty.replace(Type::Class {
                sym: owner,
                args: Vec::new(),
            });
            let conv = self.conv_at(st, bin, &scope, &declared.ty, 0);
            self.self_ty = outer;
            let Some(target) = conv else {
                trace(format_args!(
                    "{}#{name}: inherited alias {:?} does not convert",
                    hit.owner, declared.ty
                ));
                return None;
            };
            let id = st.alloc(name, owner, SymKind::TypeMember, Flags::EMPTY, "");
            st.get_mut(id).ty = target;
            st.get_mut(id).is_type_alias = true;
            st.get_mut(owner).members.push(id);
            self.invalidate_type_member_caches(name);
            id
        };
        if declared.has(pflags::PROTECTED) {
            st.get_mut(id).flags = st.get(id).flags.with(Flags::PROTECTED);
        }
        trace(format_args!(
            "{}#{name}: inherited type member installed for a subclass",
            hit.owner
        ));
        Some(id)
    }

    /// The enclosing instance named by a member object or a result type's
    /// explicit declaring-this prefix. A matching erased class is insufficient.
    pub(crate) fn member_module_owner(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        receiver: SymbolId,
        name: &str,
    ) -> Option<SymbolId> {
        let full = st
            .get(receiver)
            .jvm_name
            .trim_end_matches('$')
            .replace('/', ".");
        let module = st.get(receiver).flags.contains(Flags::MODULE);
        let (hits, _) = self.sigs.lookup(&mut BinSource(bin), &full, module, name);
        for hit in hits {
            if hit.member.kind == MemberKind::Module {
                return self.ensure_class(st, bin, &hit.owner, false);
            }
            if let Some(SigType::This(owner)) = &hit.member.result_prefix {
                if *owner == hit.owner {
                    return self.ensure_class(st, bin, owner, false);
                }
            }
        }
        None
    }

    /// `C.this.In` from a pickle: the `THIStpe` prefix of a `TypeRef` to a
    /// nested class, which `SigType` has no room for (`sym.rs` records it
    /// beside the member as `alias_prefix` / `result_prefix`). Attached as
    /// the view prefix `ThisType(C)` (`prefix.rs`) when the type names a
    /// class nested in a class: slick's `type Table[T] = JdbcProfile.this.Table[T]`
    /// and `val api: JdbcProfile.this.API` are what gitbucket's every table
    /// reaches its superclass and its enclosing instance through.
    fn with_pickled_this_prefix(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        this_prefix: Option<&str>,
        ty: Type,
    ) -> Type {
        let Some(owner_name) = this_prefix else {
            return ty;
        };
        let inner = |st: &SymbolTable, t: &Type| matches!(t, Type::Class { sym, .. } if st.is_inner_class_of_class(*sym));
        let core_is_inner = match &ty {
            Type::Method { ret, .. } => inner(st, ret),
            t => inner(st, t),
        };
        if !core_is_inner {
            return ty;
        }
        let Some(c) = self.ensure_class(st, bin, owner_name, false) else {
            return ty;
        };
        let pre = Type::ThisType(c);
        match ty {
            Type::Method { paramss, ret } => Type::Method {
                paramss,
                ret: Box::new(crate::prefix::with_prefix(*ret, pre)),
            },
            t => crate::prefix::with_prefix(t, pre),
        }
    }

    /// An alias may project an inner class from an explicit class type,
    /// rather than from its own enclosing instance. Keep that widened
    /// prefix so selecting the alias cannot capture the selection's `this`.
    fn with_pickled_alias_projection(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        scope: &HashMap<String, Type>,
        owner: SymbolId,
        name: &str,
        ty: Type,
    ) -> Type {
        if !matches!(&ty, Type::Class { sym, .. } if st.is_inner_class_of_class(*sym)) {
            return ty;
        }
        let prefix = st
            .binary_alias_prefixes
            .get(&(owner, name.to_string()))
            .cloned();
        let Some(prefix @ SigType::Ref { .. }) = prefix else {
            return ty;
        };
        match self.conv_at(st, bin, scope, &prefix, 0) {
            Some(pre) => crate::prefix::with_prefix(ty, pre),
            None => ty,
        }
    }

    /// `type T[tps] = U` from a pickle, as the type it stands for.
    ///
    /// An alias is *transparent*: a nullary one is simply its right-hand side,
    /// with no symbol of its own. That matters here — `type Tree =
    /// universe.Tree` names an abstract type member, and giving the alias a
    /// `TypeMember` symbol of its own would make `c.Tree` an opaque type that
    /// conforms to nothing rather than the `Trees.Tree` it is.
    ///
    /// A *parameterised* alias does need a symbol, to carry the parameters
    /// `expand_applied_hk_alias` substitutes at each use.
    fn install_type_alias(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        owner: SymbolId,
        name: &str,
        ty: &SigType,
        this_prefix: Option<&str>,
    ) -> Option<Type> {
        let owner_name = st
            .get(owner)
            .jvm_name
            .trim_end_matches('$')
            .replace('/', ".");
        // Only a member of `owner` itself is this alias: the pickle hit named
        // the class that *declares* the alias, so a same-named declaration
        // reached through one of its parents is the deferred member this
        // alias overrides, not the alias.
        if let Some(id) = st
            .type_members_named(owner, name)
            .into_iter()
            .find(|&s| st.get(s).kind == SymKind::TypeMember && st.get(s).owner == owner)
        {
            return Some(st.type_member_as_seen(id));
        }
        let (tps, rhs) = match ty {
            SigType::Poly { tparams, result } => (tparams.clone(), (**result).clone()),
            other => (Vec::new(), other.clone()),
        };
        let mut owner_scope: HashMap<String, Type> = st
            .get(owner)
            .tparams
            .iter()
            .map(|tp| (st.get(*tp).name.clone(), Type::TypeParam(*tp)))
            .collect();
        for referenced in mentioned(&rhs) {
            if !referenced.contains('.')
                && !owner_scope.contains_key(&referenced)
                && !tps.iter().any(|tp| tp.name == referenced)
                && referenced != name
            {
                if let Some(member) =
                    self.abstract_type_member(st, bin, &format!("{owner_name}.{referenced}"), 0)
                {
                    owner_scope.insert(referenced, member);
                }
            }
        }
        if tps.is_empty() {
            let outer = self.self_ty.replace(Type::Class {
                sym: owner,
                args: Vec::new(),
            });
            let conv = self.conv_at(st, bin, &owner_scope, &rhs, 0);
            self.self_ty = outer;
            if conv.is_none() {
                trace(format_args!(
                    "{owner_name}#{name}: type alias right-hand side {rhs:?} does not convert"
                ));
            }
            let Some(target) = conv else {
                return None;
            };
            let target = self.with_pickled_this_prefix(st, bin, this_prefix, target);
            let target =
                self.with_pickled_alias_projection(st, bin, &owner_scope, owner, name, target);
            // Only a plain class or an existing type member can itself bind
            // an imported type name. Keep a declaration for other aliases:
            // importing `type Positive = Greater[Nat._0]` must retain the
            // argument, just as a path-dependent class retains its prefix.
            if !matches!(&target, Type::Class { args, .. } if args.is_empty())
                && !matches!(&target, Type::TypeMember(_))
            {
                let id = st.alloc(name, owner, SymKind::TypeMember, Flags::EMPTY, "");
                st.get_mut(id).ty = target;
                st.get_mut(id).is_type_alias = true;
                st.get_mut(owner).members.push(id);
                self.invalidate_type_member_caches(name);
                return Some(Type::TypeMember(id));
            }
            return Some(target);
        }
        // Owned but not yet a member: a right-hand side that will not convert
        // must leave the owner exactly as it was.
        let id = st.alloc(name, owner, SymKind::TypeMember, Flags::EMPTY, "");
        let mut scope = owner_scope;
        let mut tparams = Vec::new();
        for tp in &tps {
            let t = st.alloc(&tp.name, id, SymKind::TypeParam, Flags::EMPTY, "");
            st.get_mut(t).ty = Type::TypeParam(t);
            set_tparam_kind(st, t, tp);
            scope.insert(tp.name.clone(), Type::TypeParam(t));
            tparams.push(t);
        }
        st.get_mut(id).tparams = tparams;
        st.get_mut(id).is_type_alias = true;
        // The right-hand side is written in the *declaring* class's
        // vocabulary, the same way an abstract member's bound is.
        let outer = self.self_ty.replace(Type::Class {
            sym: owner,
            args: Vec::new(),
        });
        let conv = self.conv_at(st, bin, &scope, &rhs, 0);
        self.self_ty = outer;
        let Some(target) = conv else {
            trace(format_args!(
                "{owner_name}#{name}: type alias right-hand side {rhs:?} does not convert"
            ));
            return None;
        };
        let target = self.with_pickled_this_prefix(st, bin, this_prefix, target);
        let target = self.with_pickled_alias_projection(st, bin, &scope, owner, name, target);
        st.get_mut(id).ty = target;
        st.get_mut(id).is_type_alias = true;
        st.get_mut(owner).members.push(id);
        self.invalidate_type_member_caches(name);
        trace(format_args!("type alias {owner_name}.{name}"));
        Some(Type::TypeMember(id))
    }

    /// Read the receiver's substituted declarations only. Callers already
    /// holding inherited members must not install fallback members on unrelated
    /// ancestors or companions when this receiver's pickle cannot supply them.
    pub(crate) fn complete_on_class(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        class_sym: SymbolId,
        name: &str,
    ) -> Vec<SymbolId> {
        self.complete_named(st, bin, class_sym, name, false)
    }

    /// The pickled flags of the class (or module class) `full`, or `0` when
    /// there is no pickle to read them from.
    fn declared_class_flags(&mut self, bin: &mut BinaryIndex, full: &str, is_module: bool) -> u64 {
        let mut src = BinSource(bin);
        self.sigs
            .class_sig(&mut src, full, is_module)
            .map(|sig| sig.flags)
            .unwrap_or(0)
    }

    /// `synthetic_ok` is set only when fetching a `$default$` getter, which is
    /// synthetic by construction and would otherwise be filtered out.
    fn complete_named(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        class_sym: SymbolId,
        name: &str,
        synthetic_ok: bool,
    ) -> Vec<SymbolId> {
        // A nested Scala class normally has no ScalaSignature of its own:
        // scalac puts the pickle for it on the enclosing top-level class.
        // The nested classfile can carry only an empty `Scala` marker while
        // the enclosing top-level class carries the nested declaration.
        // `pickle_readable` intentionally remains the cheap class-only
        // predicate used by callers without a BinaryIndex; this lookup has
        // the index, so admit a nested class when its enclosing pickle can be
        // found.
        let nested_pickle = if class_sym.is_none() {
            false
        } else {
            self.nested_pickle_readable(st, bin, class_sym)
        };
        if class_sym.is_none()
            || name.is_empty()
            || (!self.pickle_readable(st, class_sym) && !nested_pickle)
        {
            return Vec::new();
        }
        if !self.tried.insert((class_sym.0, name.to_string())) {
            return st
                .get(class_sym)
                .members
                .iter()
                .copied()
                .filter(|&m| st.get(m).name == name)
                .collect();
        }
        // nsc keeps operator names encoded all the way through: `SetOps`
        // pickles `&` as `$amp`, and the classfile declares `$amp` too. So the
        // encoded name is what both the pickle lookup and the descriptor
        // search use, while the symbol we install keeps the source name.
        let jvm_member = scala_rs_pickle::names::encode_method_name(name);
        if !jvm_member
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
        {
            trace(format_args!("{name}: not encodable as a JVM method name"));
            return Vec::new();
        }
        let sym = st.get(class_sym);
        if !sym.is_class_like() {
            return Vec::new();
        }
        let internal = sym.jvm_name.clone();
        // Scoped to the standard library, plus any class `adopt_binary_class`
        // has taken over: those two are the pickles the typer reads. A plain
        // Java classfile on `-cp` has no pickle at all and keeps its own path
        // in through `install_java_class`.
        if !self.pickle_readable(st, class_sym) && !nested_pickle {
            return Vec::new();
        }
        let is_module = sym.kind == SymKind::ModuleClass;
        // A pickle names a nested class `Outer.Inner`; its JVM name is
        // `Outer$Inner`. Without undoing that, `Constants$ConstantExtractor`
        // -- the receiver of `u.Constant(1)` -- has no pickle and so no
        // members at all. The `$` form is tried first, because a `$` in a
        // top-level name is part of that name.
        let plain = internal.trim_end_matches('$').replace('/', ".");
        // Whether the pickle answered to the *nested* spelling, which is what
        // says this class really is one class's member and not a top-level
        // name that happens to contain a `$`.
        let mut nested = false;
        let full = if self.has_pickle(bin, &plain, is_module) {
            plain
        } else {
            let dotted = scala_rs_pickle::names::nested_to_dotted(&plain);
            if dotted != plain && self.has_pickle(bin, &dotted, is_module) {
                nested = true;
                dotted
            } else {
                plain
            }
        };

        self.attach_parents(st, bin, class_sym, &full, is_module);

        let (hits, _errs) = {
            let mut src = BinSource(bin);
            self.sigs.lookup(&mut src, &full, is_module, &jvm_member)
        };
        if hits.is_empty() {
            trace(format_args!("{full}#{name}: not found in any pickle"));
            return Vec::new();
        }

        // The receiver's own type parameters are the vocabulary the looked-up
        // types are already expressed in.
        let mut class_scope: HashMap<String, Type> = HashMap::new();
        // A class nested in a *generic* one writes its members in terms of the
        // outer class's parameters: `class OrderingOps(lhs: T) { def <(rhs: T)
        // }` inside `trait Ordering[T]`. `OrderingOps` has no parameter of its
        // own, so `T` was a name nothing in scope mapped and every member of
        // it failed to install --
        // `value < is not a member of Ordering$OrderingOps` for what the
        // source wrote as `import seq.integral._; increment < zero`. The outer
        // symbol is the one the parameters belong to, so its own are what the
        // members are read at; the prefix the value was reached through is
        // what later substitutes them (`Typer::at_import_prefix_of`).
        if nested {
            if let Some((outer, _)) = internal.trim_end_matches('$').rsplit_once('$') {
                if let Some(o) = crate::classpath::find_by_jvm(st, outer) {
                    for tp in &st.get(o).tparams {
                        class_scope.insert(st.get(*tp).name.clone(), Type::TypeParam(*tp));
                    }
                }
            }
        }
        // An inner class's own parameter shadows the outer's of the same name.
        for tp in &st.get(class_sym).tparams {
            class_scope.insert(st.get(*tp).name.clone(), Type::TypeParam(*tp));
        }
        // Nested completion (a `$default$n` getter) re-enters here, so the
        // outer receiver's meaning of `this.type` is saved and restored.
        let saved_self = self.self_ty.replace(Type::Class {
            sym: class_sym,
            args: st
                .get(class_sym)
                .tparams
                .iter()
                .map(|t| Type::TypeParam(*t))
                .collect(),
        });

        // What the *classfile reader* left behind for a `$default$n` getter,
        // so the pickled signature can replace it rather than stand next to
        // it. `adopt_binary_class` does exactly this for every ordinary member
        // name, but it skips any name containing a `$` -- so a default getter,
        // which is only ever reached through this function's `synthetic_ok`
        // path, kept both copies. `ToolBox.typecheck$default$2` was then the
        // crude `(): Object` from the class file *and* the pickled `():
        // TypecheckMode`, and filling in the default at `tb.typecheck(t)` was
        // "ambiguous overload for typecheck$default$2 with arguments ()".
        // An inherited stable accessor is absent from this class's own
        // pickle, so adoption cannot replace its erased mixin forwarder.
        // Complete it here too: a singleton return otherwise leaves both
        // `Class(module)` and `ModuleRef(module)` as competing alternatives.
        let stable_accessor = hits.iter().any(|hit| hit.member.has(pflags::STABLE));
        let stale: Vec<SymbolId> =
            if (synthetic_ok && is_default_getter(&jvm_member)) || stable_accessor {
                st.get(class_sym)
                    .members
                    .iter()
                    .copied()
                    .filter(|&m| {
                        let s = st.get(m);
                        s.kind == SymKind::Method
                            && s.name == name
                            && s.pickled_origin.is_empty()
                            && m.0 >= st.prelude_end
                            && (!stable_accessor
                                || (s.params.is_empty() && !s.flags.contains(Flags::STATIC)))
                    })
                    .collect()
            } else {
                Vec::new()
            };
        // A *prelude* class's `apply` is hand-written and authoritative --
        // `adopt_binary_class` refuses such a class outright for the same
        // reason. Offering the pickled `Some$.apply` from here made
        // `Some("x")("spurious")` compile (`neg/t4196`): the caller reads a
        // non-empty answer as "this receiver can be applied", and the
        // application went out as `Some$.apply("spurious")` with the receiver
        // dropped. The case-class relaxation is for the libraries on `-cp`,
        // which is where the class file's description is all there is.
        let case_synthetic_ok = !(internal.starts_with("scala/") && class_sym.0 < st.prelude_end);
        let mut installed: Vec<SymbolId> = Vec::new();
        let mut seen_shapes: Vec<(SymbolId, String, usize, Vec<Type>)> = Vec::new();
        // Members a later, more derived declaration displaced.
        let mut superseded: Vec<SymbolId> = Vec::new();
        for hit in &hits {
            // A projection such as U#ModuleSymbol keeps U in its encoded name.
            // Supply the declaring parent's arguments too: ordinary references
            // were already substituted by lookup, but a projection still needs
            // the receiver's meaning of U (e.g. Mirror[JavaUniverse.this.type]).
            let mut class_scope = class_scope.clone();
            if let Some(subst) = self.alias_owner_subst(st, bin, &hit.owner) {
                for (name, ty) in subst {
                    if let Some(ty) = self.conv_at(st, bin, &class_scope, &ty, 0) {
                        class_scope.entry(name).or_insert(ty);
                    }
                }
            }
            let m = &hit.member;
            // A nested `object`. Not a signature at all: what the class file
            // carries is an accessor returning the module class, and the
            // module's own members are read from its own pickle when someone
            // asks for one. See `install_nested_module`.
            // Case companions are synthetic but remain source-level API.
            // Their owner and instance/static ABI still come from the pickle.
            let case_companion = m.kind == MemberKind::Module
                && m.has(pflags::SYNTHETIC)
                && !m.has(pflags::PRIVATE | pflags::LOCAL)
                && self
                    .sigs
                    .class_sig(&mut BinSource(bin), &format!("{}.{name}", hit.owner), false)
                    .is_ok_and(|sig| sig.flags & pflags::CASE != 0);
            if m.kind == MemberKind::Module && (m.is_public_api() || case_companion) {
                let owner = hit.owner.clone();
                if let Some(id) =
                    self.install_nested_module(st, bin, class_sym, &owner, hit.owner_module, name)
                {
                    if m.has(pflags::IMPLICIT) {
                        st.get_mut(id).flags = st.get(id).flags.with(Flags::IMPLICIT);
                        let result = match &st.get(id).ty {
                            Type::Method { ret, .. } => (**ret).clone(),
                            ty => ty.clone(),
                        };
                        if let Some(module) = st.class_sym_of(&result) {
                            self.ensure_parents(st, bin, module);
                        }
                    }
                    // The same object can be reached through more than one
                    // hit (a trait inherited twice over); one accessor is
                    // one member, not an overload of itself.
                    if !installed.contains(&id) {
                        installed.push(id);
                    }
                }
                continue;
            }
            // A macro def (`MACRO` in the pickled flags). It leaves no
            // bytecode at all -- nsc's own rule, `docs/macros.md` §1.1 -- so
            // the general path below, which finds the *method* by matching an
            // erased descriptor against the owner's real classfile, can never
            // succeed for one; the failure was observed as
            // `scala/reflect/runtime/package$#currentMirror/0: no unambiguous
            // erased descriptor (want [])`, which is 0 candidates (no
            // bytecode) read as "not unambiguous", not evidence of a
            // collision. `install_known_macro` supplies the handful this
            // module knows a real, honest binding for; anything else falls
            // through and is declined exactly as before.
            if m.kind == MemberKind::Def && m.has(pflags::MACRO) {
                let id = match &m.macro_impl {
                    Some(mi) => self.install_pickled_macro(
                        st,
                        bin,
                        class_sym,
                        &internal,
                        name,
                        &m.ty,
                        mi,
                        &class_scope,
                    ),
                    None => self.install_known_macro(st, bin, class_sym, &internal, name, &m.ty),
                };
                if let Some(id) = id {
                    // IMPLICIT belongs to the macro declaration, not to its
                    // parameter clauses. Preserve it just as for ordinary defs.
                    if m.has(pflags::IMPLICIT) {
                        st.get_mut(id).flags = st.get(id).flags.with(Flags::IMPLICIT);
                    }
                    st.get_mut(id).flags = st.get(id).flags.with(ctor_access_flags(m));
                    st.get_mut(id).private_within = ctor_access_within(m);
                    if !installed.contains(&id) {
                        installed.push(id);
                    }
                }
                continue;
            }
            if m.kind != MemberKind::Def && m.kind != MemberKind::Val {
                continue;
            }
            // Only a synthetic `copy` needs its declaring class's flags; the
            // lookup is skipped for every other member.
            let case_copy = case_synthetic_ok
                && m.name == "copy"
                && m.has(pflags::SYNTHETIC)
                && m.is_case_copy(self.declared_class_flags(bin, &hit.owner, hit.owner_module));
            if !m.is_inheritable_api()
                && !(m.has(pflags::PRIVATE)
                    && hit.owner == full
                    && !m.has(pflags::BRIDGE)
                    && !m.has(pflags::SYNTHETIC))
                && !(case_synthetic_ok && m.is_case_synthetic())
                && !case_copy
                && !implicit_class_conversion_from(st, class_sym, &hit.owner, m)
                && !(synthetic_ok && is_default_getter(&m.name))
            {
                continue;
            }
            let Some(mut shape) = read_shape(&m.ty) else {
                trace(format_args!(
                    "{internal}#{name}: unreadable signature shape"
                ));
                continue;
            };
            shape.implicit = m.has(pflags::IMPLICIT);
            let Some(shape) = pin_undetermined_tparams(shape) else {
                trace(format_args!(
                    "{internal}#{name}: type parameter appears only in an implicit \
                     clause and has no lower bound to pin it to"
                ));
                continue;
            };
            if let Some(id) = self.install(
                st,
                bin,
                class_sym,
                &internal,
                name,
                &jvm_member,
                &hit.owner,
                hit.owner_module,
                &shape,
                &class_scope,
                m.result_prefix.as_ref(),
                &mut seen_shapes,
                &mut superseded,
            ) {
                // Keep access from the Scala declaration when replacing the
                // eager classpath signature. JVM access alone cannot express
                // protected[p], private[p], or private[this].
                st.get_mut(id).flags = st.get(id).flags.with(ctor_access_flags(m));
                st.get_mut(id).private_within = ctor_access_within(m);
                if m.has(pflags::LOCAL) {
                    st.get_mut(id).flags = st.get(id).flags.with(Flags::LOCAL);
                }
                // Preserve the declaration-side stable path of a value or
                // method result. It may be needed later when this inherited
                // member is selected through a concrete profile.
                if let Some(prefix) = &m.result_prefix {
                    trace(format_args!(
                        "{internal}#{name}: recording result prefix for {id:?}: {prefix:?}"
                    ));
                    self.result_prefixes.insert(id, prefix.clone());
                }
                // A constructor's "result" is the class itself; its hidden
                // outer slot is the backend's (`hidden_outer_desc`), not a
                // prefix to record.
                if let (Some(SigType::This(c)), false) = (&m.result_prefix, name == "<init>") {
                    let c = c.clone();
                    let ty = std::mem::replace(&mut st.get_mut(id).ty, Type::NoType);
                    let ty = self.with_pickled_this_prefix(st, bin, Some(&c), ty);
                    st.get_mut(id).ty = ty;
                }
                st.get_mut(id).parameterless_method = Some(shape.clauses.is_empty());
                // A `val`'s accessor is stable; `ident_is_stable` /
                // `member_is_stable` read this flag to accept it as a path
                // prefix in type position (`Resource.ExitCase`) and in
                // `stable identifier required` checks generally.
                //
                // `MemberKind::Val` is *not* the signal: nsc's pickle marks a
                // package object's `val` accessor with `pflags::METHOD` too
                // (it is a real zero-arg method once compiled), so `read()`
                // (`crates/pickle/src/sym.rs`) classifies it as `Def`, same as
                // an ordinary `def`. `pflags::STABLE` is the flag nsc itself
                // uses to tell the two apart, and is set regardless of which
                // `MemberKind` the entry came out as.
                if m.has(pflags::STABLE) {
                    st.get_mut(id).flags = st.get(id).flags.with(Flags::ACCESSOR);
                }
                // A declaration, not a definition. The class file cannot say
                // so for a trait -- every member of an interface bar its
                // `default` methods is `ACC_ABSTRACT` -- so the pickle is the
                // only place it is written down. `Symbol::deferred_method`
                // rather than `Flags::ABSTRACT` for the reason recorded on
                // that field: `override_check::modifiers_are_known` withholds
                // every modifier-shaped diagnostic for pickled members, and
                // setting the flag here would turn them all on at once.
                if m.has(pflags::FINAL) {
                    st.get_mut(id).flags = st.get(id).flags.with(Flags::FINAL);
                }
                st.get_mut(id).deferred_method = m.has(pflags::DEFERRED);
                installed.push(id);
            }
        }
        self.self_ty = saved_self;
        // A member a derived declaration displaced is detached from the class;
        // handing it back would put it straight into overload resolution as an
        // alternative nothing can reach.
        if !superseded.is_empty() {
            installed.retain(|m| !superseded.contains(m));
        }
        // Replace the raw JVM view of each signature that was completed.
        // Return descriptors may be erased value classes or existentials;
        // keeping both views creates a spurious overload.
        let mut stale = stale;
        stale.extend(st.get(class_sym).members.iter().copied().filter(|id| {
            let raw = st.get(*id);
            raw.flags.contains(Flags::JAVA)
                && raw.pickled_origin.is_empty()
                && id.0 >= st.prelude_end
                && !installed.contains(id)
                && installed.iter().any(|new| {
                    st.get(*new).name == raw.name && st.get(*new).jvm_name == raw.jvm_name
                })
        }));
        if !installed.is_empty() && !stale.is_empty() {
            drop_stale_members(st, class_sym, &stale, &installed);
            trace(format_args!(
                "{full}#{name}: replaced {} class-file accessor(s)",
                stale.len()
            ));
        }
        trace(format_args!(
            "{full}#{name}: supplied {} overload(s)",
            installed.len()
        ));
        installed
    }

    /// Supply the small, fixed set of macro defs this module knows a real
    /// implementation binding for, by full name.
    ///
    /// A macro def's declaration survives in the `ScalaSignature` (that is
    /// how `hits` found it at all), but its *implementation* is looked up a
    /// different way: nsc reads a pickled `@scala.reflect.macros.internal
    /// .macroImpl(...)` annotation naming the class/method to call
    /// (`docs/macros.md` §1.1). [`PickleSupply::install_pickled_macro`] reads
    /// that annotation and is the general path; this one is for the macros
    /// that do not have a usable one. `scala.reflect.runtime.currentMirror`
    /// is the case: it is one of nsc's
    /// **fast-track** macros (`scala.tools.reflect.FastTrack`), which the
    /// compiler recognises by the macro symbol's *full name* and never
    /// consults the pickled annotation for -- the annotation actually present
    /// on the real classfile is a placeholder (`macro ???`), not a usable
    /// reference.
    ///
    /// So this supplies exactly the bindings scala-rs knows, the same way
    /// nsc's own `FastTrack` table does: by name, hand-verified against the
    /// real jar. `scala.reflect.runtime.Macros.currentMirror(c:
    /// blackbox.Context): c.Expr[universe.Mirror]` is a real, ordinary
    /// blackbox macro implementation with real bytecode (confirmed with
    /// `javap scala.reflect.runtime.Macros$` against scala-reflect.jar
    /// 2.13.16; see `docs/notes/macro-reflect-and-reify.md`). Nothing here
    /// invents behaviour: the return type is read from this same pickle, and
    /// the existing JVM-bridge macro engine (`crates/typer/src/expand.rs`)
    /// decides at the call site whether it can actually expand the binding,
    /// reporting its own "macro expansion is not implemented" diagnostic
    /// (`Typer::report_macro_calls`) when some part of the call -- `c
    /// .reifyEnclosingRuntimeClass` chief among them -- is not wired up
    /// there yet. Either way the name is never silently accepted: it becomes
    /// visible with its real type, and any actual use goes through the same
    /// expansion-or-diagnose path as a source-level macro def.
    fn install_known_macro(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        class_sym: SymbolId,
        internal: &str,
        name: &str,
        ty: &SigType,
    ) -> Option<SymbolId> {
        let (impl_class, impl_method) = match (internal, name) {
            ("scala/reflect/runtime/package$", "currentMirror") => {
                ("scala/reflect/runtime/Macros$", "currentMirror")
            }
            _ => return None,
        };
        let shape = read_shape(ty)?;
        // Only the zero-argument, non-generic shape nsc actually declares for
        // `currentMirror` is handled; anything else is left alone rather than
        // guessed at.
        if !shape.clauses.is_empty() || !shape.tparams.is_empty() {
            trace(format_args!(
                "{internal}#{name}: known macro binding does not match the pickled shape"
            ));
            return None;
        }
        if let Some(existing) = st
            .get(class_sym)
            .members
            .iter()
            .copied()
            .find(|&s| st.get(s).name == name)
        {
            return Some(existing);
        }
        let outer = self.self_ty.replace(Type::Class {
            sym: class_sym,
            args: Vec::new(),
        });
        let ret = self.conv_at(st, bin, &HashMap::new(), &shape.ret, 0);
        self.self_ty = outer;
        let Some(ret) = ret else {
            trace(format_args!(
                "{internal}#{name}: known macro binding's return type does not convert"
            ));
            return None;
        };
        let id = st.alloc(name, class_sym, SymKind::Method, Flags::FINAL, "");
        st.get_mut(id).ty = Type::Method {
            paramss: Vec::new(),
            ret: Box::new(ret),
        };
        st.get_mut(id).macro_impl = Some(MacroBinding {
            pickle: None,
            is_bundle: false,
            impl_class: impl_class.to_string(),
            impl_method: impl_method.to_string(),
            blackbox: true,
            tag_params: 0,
            expr_args: Vec::new(),
            tag_targs: Vec::new(),
        });
        st.get_mut(class_sym).members.push(id);
        // Same as `install_pickled_macro`: this is the gate on the typer
        // walking applications looking for something to expand at all
        // (`Check::type_expr`). Without it a run whose only macro is this one
        // never attempted an expansion, so every `currentMirror` was reported
        // as "cannot expand" with no reason attached -- because nothing had
        // tried.
        self.supplied_macro_def = true;
        trace(format_args!(
            "{internal}#{name}: supplied as a known macro binding ({impl_class}.{impl_method})"
        ));
        Some(id)
    }

    /// Supply a macro def a jar's pickle declares, with the implementation
    /// reference nsc baked into its `@macroImpl` annotation.
    ///
    /// This is the general form of [`PickleSupply::install_known_macro`], and
    /// the reason gitbucket could not call slick: `lazy val Issues =
    /// TableQuery[Issues]` is `TableQuery.apply[Issues]`, and the alternative
    /// it means -- `def apply[E]: TableQuery[E] = macro …` -- is a macro def,
    /// so nothing supplied it at all. The other alternative,
    /// `apply[E](cons: Tag => E)`, was then the only member of that name, the
    /// reference resolved to *it*, and every query method was looked for on
    /// the un-applied method type `((Tag) => Issues)TableQuery[Issues]`.
    ///
    /// Nothing is invented. The declared type is this pickle's own, and
    /// [`MacroBinding`] is read out of the annotation nsc wrote:
    /// `className` / `methodName` name the implementation, and `signature`
    /// records the fingerprint of each of its parameters, from which the two
    /// facts the expander needs -- which arguments are `c.Expr`, and how many
    /// `WeakTypeTag`s the trailing clause takes -- follow directly. The
    /// implementation itself is never read here: whether it can actually be
    /// run is decided at the call site by the JVM bridge
    /// (`crates/typer/src/expand.rs`), which reports its own diagnostic
    /// through [`crate::check::Typer::report_macro_calls`] when it cannot.
    /// A macro that cannot be expanded is therefore an error, never a
    /// silently accepted call.
    ///
    /// `is_blackbox` is carried into the binding rather than required:
    /// `Typer::expand_macro_application` types a whitebox expansion with no
    /// expected type and keeps the type it came out with, which is what nsc's
    /// `macroExpandApply` does for a `WhiteboxExpansion`.
    ///
    /// Macro bundles carry Context in their constructor instead of the
    /// implementation method's first argument clause.
    #[allow(clippy::too_many_arguments)]
    fn install_pickled_macro(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        class_sym: SymbolId,
        internal: &str,
        name: &str,
        ty: &SigType,
        mi: &PickledMacroImpl,
        class_scope: &HashMap<String, Type>,
    ) -> Option<SymbolId> {
        let Some((tag_indices, expr_args)) = macro_signature_shape(&mi.signature, mi.is_bundle)
        else {
            trace(format_args!(
                "{internal}#{name}: the pickled macro signature {:?} is not a shape \
                 this expander knows",
                mi.signature
            ));
            return None;
        };
        let tag_params = tag_indices.len();
        let shape = read_shape(ty)?;
        if shape.arity() != expr_args.len() {
            trace(format_args!(
                "{internal}#{name}: the macro def takes {} argument(s) and the \
                 implementation {} -- declining rather than guessing",
                shape.arity(),
                expr_args.len()
            ));
            return None;
        }
        // Allocated ownerless so that a conversion failure leaves nothing
        // behind, the way `install` does it.
        let m = st.alloc(name, SymbolId::NONE, SymKind::Method, Flags::EMPTY, "");
        let mut scope = class_scope.clone();
        let mut tparams = Vec::new();
        for tp in &shape.tparams {
            let id = alloc_shape_tparam(st, m, tp);
            scope.insert(tp.name.clone(), Type::TypeParam(id));
            tparams.push(id);
        }
        for (tp, id) in shape.tparams.iter().zip(tparams.iter().copied()) {
            self.resolve_shape_tparam_bounds(st, bin, &scope, tp, id);
        }
        let mut paramss_ty: Vec<Vec<Type>> = Vec::new();
        let mut paramss_sym: Vec<Vec<SymbolId>> = Vec::new();
        for clause in &shape.clauses {
            let mut tys = Vec::new();
            let mut syms = Vec::new();
            for p in &clause.params {
                let Some(mut t) = self.conv(st, bin, &scope, &p.ty) else {
                    trace(format_args!(
                        "{internal}#{name}: parameter {} has an unmappable type {:?}",
                        p.name, p.ty
                    ));
                    return None;
                };
                if p.by_name && !matches!(t, Type::ByName(_)) {
                    t = Type::ByName(Box::new(t));
                }
                let flags = if clause.implicit {
                    Flags::PARAM.with(Flags::IMPLICIT)
                } else {
                    Flags::PARAM
                };
                let ps = st.alloc(
                    scala_rs_pickle::names::decode_method_name(&p.name),
                    m,
                    SymKind::Term,
                    flags,
                    "",
                );
                st.get_mut(ps).ty = t.clone();
                tys.push(t);
                syms.push(ps);
            }
            paramss_ty.push(tys);
            paramss_sym.push(syms);
        }
        let Some(ret) = self.conv(st, bin, &scope, &shape.ret) else {
            trace(format_args!(
                "{internal}#{name}: unmappable result type {:?}",
                shape.ret
            ));
            return None;
        };
        // The same macro reached through two parents is one member, not an
        // overload of itself; and a second run of the same completion must
        // not install a second copy beside the first.
        let origin = format!("{}.{}", mi.class_name, mi.method_name);
        if let Some(&existing) = st.get(class_sym).members.iter().find(|&&s| {
            let e = st.get(s);
            e.name == name && e.macro_impl.as_ref().is_some_and(|b| b.origin() == origin)
        }) {
            return Some(existing);
        }
        st.get_mut(m).tparams = tparams;
        st.get_mut(m).params = paramss_sym.iter().flatten().copied().collect();
        st.get_mut(m).paramss = paramss_sym;
        st.get_mut(m).ty = Type::Method {
            paramss: paramss_ty,
            ret: Box::new(ret),
        };
        if shape.implicit {
            let f = st.get(m).flags.with(Flags::IMPLICIT);
            st.get_mut(m).flags = f;
        }
        let tag_targs =
            self.pickled_tag_targs(st, bin, m, &scope, mi, &tag_indices, internal, name);
        st.get_mut(m).macro_impl = Some(MacroBinding {
            pickle: None,
            is_bundle: mi.is_bundle,
            impl_class: mi.class_name.clone(),
            impl_method: mi.method_name.clone(),
            blackbox: mi.is_blackbox,
            tag_params,
            expr_args,
            tag_targs,
        });
        st.get_mut(m).owner = class_sym;
        st.get_mut(class_sym).members.push(m);
        self.supplied_macro_def = true;
        trace(format_args!(
            "{internal}#{name}: supplied as a pickled macro def ({origin})"
        ));
        Some(m)
    }

    /// What each `WeakTypeTag` a pickled macro implementation asks for stands
    /// for, read out of the type arguments nsc wrote on the implementation
    /// reference (`docs/macros.md` §7.22).
    ///
    /// This is the pickled half of `crates/typer/src/macros.rs`'s
    /// [`crate::check::Typer::classify_macro_targ`], and it converts in
    /// exactly the scope `install_pickled_macro` has already built: the macro
    /// def's own type parameters *and* the owning class's. That is what the
    /// list needs, because the two kinds are precisely what has to be told
    /// apart -- `def mapTo[R] = macro ShapedValue.mapToImpl[R, U]` writes one
    /// of each.
    ///
    /// An empty result means "not known" and leaves the older one-for-one rule
    /// in place; it is returned whenever a fingerprint points past the end of
    /// the written type arguments, which is a pickle this code does not
    /// understand rather than a macro it can resolve half of.
    #[allow(clippy::too_many_arguments)]
    fn pickled_tag_targs(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        macro_def: SymbolId,
        scope: &HashMap<String, Type>,
        mi: &PickledMacroImpl,
        tag_indices: &[usize],
        internal: &str,
        name: &str,
    ) -> Vec<MacroTarg> {
        if tag_indices.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::with_capacity(tag_indices.len());
        for &i in tag_indices {
            let Some(sig) = mi.targs.get(i) else {
                trace(format_args!(
                    "{internal}#{name}: the pickled macro signature asks for the type \
                     argument at {i} and the implementation reference writes {} -- \
                     falling back to the call site's own type arguments",
                    mi.targs.len()
                ));
                return Vec::new();
            };
            // Ordinary signature conversion requires full class arity. A
            // macro implementation reference instead takes the symbol's own
            // type, including its own parameters for an unapplied constructor.
            let fixed_class = match sig {
                SigType::Ref { sym, args } if args.is_empty() && !scope.contains_key(sym) => self
                    .ensure_class(st, bin, sym, false)
                    .map(|id| st.type_of_class(id)),
                _ => None,
            };
            out.push(
                match fixed_class.or_else(|| self.conv(st, bin, scope, sig)) {
                    Some(t) => crate::macros::macro_targ_of_type(st, &t, macro_def),
                    None => MacroTarg::Unresolved(sig_spelling(sig)),
                },
            );
        }
        trace(format_args!(
            "{internal}#{name}: implementation reference type arguments {out:?}"
        ));
        out
    }

    /// Supply a nested `object` a pickle declares, as a **module accessor**.
    ///
    /// `trait Exprs { object Expr { … } }` compiles to an interface method
    /// `Expr()Lscala/reflect/api/Exprs$Expr$;` plus a class file for the
    /// module itself. `complete_named` reads only `Def` and `Val` members, so
    /// a `MemberKind::Module` entry was dropped whole: `c.universe.Expr` was
    /// "value Expr is not a member of Universe" and `import c.universe._;
    /// Expr` was "not found: value Expr", both untrue -- the member is right
    /// there in the pickle (`docs/macros.md` §7.14).
    ///
    /// The accessor is installed on **`class_sym`**, the receiver the lookup
    /// started from, exactly as `install` does for an inherited `def`. That
    /// is not cosmetic: `Check::qualify_term_import` rewrites an unqualified
    /// name from `import u._` back into `u.name` by matching the *member's
    /// owner* against the import prefix's class, and a library class's
    /// pickled parents are attached one level at a time -- so an accessor
    /// parked on the declaring trait far up the linearisation is not
    /// recognised as this import's, and the backend emitted `Main$.Expr()`
    /// (`ClassCastException: Main$ cannot be cast to scala.reflect.api.Exprs`)
    /// from a compile that reported nothing.
    ///
    /// The module's own members are *not* installed here. They are read from
    /// its own pickle the first time one is asked for, which is the same
    /// on-demand path every other library member takes; the nested spelling
    /// (`scala.reflect.api.Exprs.Expr`) is what `complete_named` recovers
    /// from the `$`-joined JVM name.
    fn install_nested_module(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        class_sym: SymbolId,
        pickle_owner: &str,
        pickle_owner_module: bool,
        name: &str,
    ) -> Option<SymbolId> {
        let requested_owner = st
            .get(class_sym)
            .jvm_name
            .trim_end_matches('$')
            .replace('/', ".");
        let decl = if requested_owner == pickle_owner
            && (st.get(class_sym).kind == SymKind::ModuleClass) == pickle_owner_module
        {
            class_sym
        } else {
            self.ensure_class(st, bin, pickle_owner, pickle_owner_module)?
        };
        let decl_jvm = st.get(decl).jvm_name.trim_end_matches('$').to_string();
        if decl_jvm.is_empty() {
            return None;
        }
        let module_jvm = format!("{decl_jvm}${name}$");
        // Objects nested in static objects have a real MODULE$ field, while
        // instance member objects have an accessor on their enclosing receiver.
        // Choose from the classfile ABI instead of inventing that accessor.
        let is_static_module = self.java_class(bin, &module_jvm).is_some_and(|jc| {
            jc.fields.iter().any(|f| {
                f.name == "MODULE$" && f.access & 0x0008 != 0 && f.desc == format!("L{module_jvm};")
            })
        });
        if is_static_module {
            let mcls = self.ensure_class(st, bin, &format!("{pickle_owner}.{name}"), true)?;
            let owner = st.get(mcls).owner;
            let module = st
                .get(owner)
                .members
                .iter()
                .copied()
                .find(|&id| st.get(id).kind == SymKind::Module && st.module_class_of(id) == mcls)
                .unwrap_or_else(|| {
                    let id = st.alloc(name, class_sym, SymKind::Module, Flags::MODULE, &module_jvm);
                    st.get_mut(id).ty = Type::ModuleRef(mcls);
                    id
                });
            if !st.get(class_sym).members.contains(&module) {
                st.get_mut(class_sym).members.push(module);
            }
            return Some(module);
        }
        // The class file has to be there. Without this the accessor would
        // name a class that does not exist and the call would fail to link at
        // run time rather than at compile time.
        if bin.find_class(&module_jvm).ok().flatten().is_none() {
            trace(format_args!(
                "{pickle_owner}#{name}: no class file {module_jvm} for the nested object"
            ));
            return None;
        }
        let mcls = match crate::classpath::find_by_jvm(st, &module_jvm) {
            Some(id) => id,
            None => {
                // Allocated ownerless and then re-owned, the way
                // `ensure_tag_module` does it: entering it in the declaring
                // class's member list would put a second `Expr` next to the
                // accessor, and member lookup would have to choose.
                let id = st.alloc(
                    format!("{name}$"),
                    SymbolId::NONE,
                    SymKind::ModuleClass,
                    Flags::MODULE.with(Flags::FINAL),
                    &module_jvm,
                );
                st.get_mut(id).owner = decl;
                st.get_mut(id).ty = Type::ModuleRef(id);
                st.get_mut(id).parents = vec![Type::AnyRef];
                self.stubs.insert(module_jvm.clone(), id);
                id
            }
        };
        // A directory scan may have attached this stub to the declaring
        // class's companion. Its instance accessor belongs to the actual
        // pickle owner; loading MODULE$ would fail for this classfile ABI.
        st.get_mut(mcls).owner = decl;
        st.get_mut(mcls).flags.set(Flags::JAVA, false);
        st.get_mut(mcls).flags.set(Flags::STATIC, false);
        if module_jvm == Self::EXPR_MODULE {
            self.install_expr_apply(st, bin, mcls);
        }
        let want = Type::Method {
            paramss: Vec::new(),
            ret: Box::new(Type::ModuleRef(mcls)),
        };
        // An accessor already reachable from here may be **useless**. Reading
        // `Exprs.class` as a plain class file (`adopt_binary_class`) installs
        // `def Expr(): Exprs$Expr$` out of the JVM descriptor, and nothing
        // had entered a symbol for `Exprs$Expr$` -- so the result type stayed
        // an unresolved `Type::Named`, `class_sym_of` answered `None`, and
        // `c.universe.Expr.apply` read "value apply is not a member of
        // Exprs$Expr$". Every such copy is repaired, wherever it sits;
        // `materialize::resolve_named_tags` does the same for the two tag
        // companions. A result type that *did* resolve is left exactly as it
        // is -- this adds precision, it never takes a member away.
        let mut here = SymbolId::NONE;
        for id in st.lookup_member(class_sym, name) {
            if !matches!(st.get(id).kind, SymKind::Method | SymKind::Term) {
                continue;
            }
            // Only a *method* is repaired: a `Term` of this name is a field
            // some other path installed, and rewriting its type into a method
            // type would break whatever reads it.
            let resolved = st.get(id).kind != SymKind::Method
                || match &st.get(id).ty {
                    Type::Method { paramss, ret } if paramss.iter().all(|c| c.is_empty()) => {
                        st.class_sym_of(ret).is_some()
                    }
                    other => st.class_sym_of(other).is_some(),
                };
            if !resolved {
                st.get_mut(id).ty = want.clone();
                let f = st.get(id).flags.with(Flags::ACCESSOR);
                st.get_mut(id).flags = f;
                if st.get(id).jvm_name.is_empty() {
                    st.set_jvm_name(id, format!("()L{module_jvm};"));
                }
                trace(format_args!(
                    "{pickle_owner}#{name}: repaired an accessor of nested object {module_jvm}"
                ));
            }
            if st.get(id).owner == class_sym {
                here = id;
            }
        }
        if !here.is_none() {
            return Some(here);
        }
        // Where the *call* has to name the accessor. `api/JavaUniverse` is a
        // class file with `interfaces: 0` even though the trait extends
        // `api.Universe`, so `invokevirtual JavaUniverse.Expr()` resolves to
        // nothing (`NoSuchMethodError` at the first use, from a compile that
        // reported nothing). `erased_desc` walks the same frontier `install`
        // uses and says which class file really declares it, and whether the
        // hop is one the JVM can make.
        let internal = st.get(class_sym).jvm_name.clone();
        let jvm_member = scala_rs_pickle::names::encode_method_name(name);
        let found = self.erased_desc(bin, &internal, &jvm_member, &[])?;
        // `alloc` enters it in `class_sym`'s member list, which is what
        // `lookup_member` and `qualify_term_import` both read.
        let acc = st.alloc(
            name,
            class_sym,
            SymKind::Method,
            // An `object` is a stable path: `ident_is_stable` and
            // `member_is_stable` read `ACCESSOR` to say so.
            Flags::ACCESSOR,
            found.desc.clone(),
        );
        if found.off_the_bytecode_path || found.declared_in != internal {
            st.get_mut(acc).declaring_class = found.declared_in;
            st.get_mut(acc).declaring_is_interface = found.declared_by_interface;
        }
        st.get_mut(acc).ty = want;
        trace(format_args!(
            "{pickle_owner}#{name}: supplied nested object {module_jvm}"
        ));
        Some(acc)
    }

    /// JVM name of the one nested object whose `apply` has to be written out.
    const EXPR_MODULE: &'static str = "scala/reflect/api/Exprs$Expr$";

    /// `Exprs#Expr.apply`, written out rather than read from the pickle.
    ///
    /// The pickled signature is
    ///
    /// ```text
    /// def apply[T](mirror1: Mirror[Universe.this.type], treec: TreeCreator)
    ///             (implicit tag: WeakTypeTag[T]): Expr[T]
    /// ```
    ///
    /// and `Universe.this.type` is converted against whatever class is under
    /// completion -- here the module `Expr$` itself -- so the first parameter
    /// came out `Mirror[Expr$]` and no call site could ever match it
    /// (`no matching overload for (Mirror[Expr$], TreeCreator)(WeakTypeTag[T])
    /// Exprs$Expr[T]`). `materialize::ensure_tag_module` writes `TypeTag.apply`
    /// out by hand for exactly the same reason; see `docs/macros.md` §7.10 and
    /// §7.14.
    ///
    /// This is the constructor of every `Expr`, which is what `reify { … }`
    /// expands into, so it is the one nested object that needs the treatment.
    /// The erased descriptor is written out too -- the same convention
    /// `install` uses for a library member whose Scala signature does not
    /// convert.
    fn install_expr_apply(&mut self, st: &mut SymbolTable, bin: &mut BinaryIndex, mcls: SymbolId) {
        // Whatever is there already wins, and asking the pickle for `apply`
        // must never add the unusable one next to this one.
        let fresh = self.tried.insert((mcls.0, "apply".to_string()));
        if !fresh || !st.lookup_member(mcls, "apply").is_empty() {
            return;
        }
        let Some(mirror) = self.ensure_class(st, bin, "scala.reflect.api.Mirror", false) else {
            return;
        };
        let Some(creator) = self.ensure_class(st, bin, "scala.reflect.api.TreeCreator", false)
        else {
            return;
        };
        let Some(wtt) = self.ensure_class(st, bin, "scala.reflect.api.TypeTags.WeakTypeTag", false)
        else {
            return;
        };
        let Some(expr) = self.ensure_class(st, bin, "scala.reflect.api.Exprs.Expr", false) else {
            return;
        };

        let ap = st.alloc("apply", mcls, SymKind::Method, Flags::EMPTY, "");
        let t = st.alloc("T", ap, SymKind::TypeParam, Flags::EMPTY, "");
        st.get_mut(t).ty = Type::TypeParam(t);
        st.get_mut(ap).tparams = vec![t];

        let mirror_ty = Type::Class {
            sym: mirror,
            args: vec![],
        };
        let creator_ty = Type::Class {
            sym: creator,
            args: vec![],
        };
        let tag_ty = Type::Class {
            sym: wtt,
            args: vec![Type::TypeParam(t)],
        };
        let p1 = st.alloc("mirror1", ap, SymKind::Term, Flags::PARAM, "");
        st.get_mut(p1).ty = mirror_ty.clone();
        let p2 = st.alloc("treec", ap, SymKind::Term, Flags::PARAM, "");
        st.get_mut(p2).ty = creator_ty.clone();
        // The tag clause is implicit, which is what lets a hand-written
        // `c.universe.Expr.apply[T](m, creator)` reach the materialiser
        // (`Check::materialize_tag`) for its `WeakTypeTag[T]`.
        let p3 = st.alloc(
            "evidence$1",
            ap,
            SymKind::Term,
            Flags::PARAM.with(Flags::IMPLICIT),
            "",
        );
        st.get_mut(p3).ty = tag_ty.clone();
        st.get_mut(ap).params = vec![p1, p2, p3];
        st.get_mut(ap).paramss = vec![vec![p1, p2], vec![p3]];
        st.get_mut(ap).ty = Type::Method {
            paramss: vec![vec![mirror_ty, creator_ty], vec![tag_ty]],
            ret: Box::new(Type::Class {
                sym: expr,
                args: vec![Type::TypeParam(t)],
            }),
        };
        st.set_jvm_name(
            ap,
            "(Lscala/reflect/api/Mirror;\
             Lscala/reflect/api/TreeCreator;\
             Lscala/reflect/api/TypeTags$WeakTypeTag;)\
             Lscala/reflect/api/Exprs$Expr;",
        );
        trace(format_args!("wrote out {}#apply", Self::EXPR_MODULE));
    }

    /// The erased parameter descriptors the *declaring* class wrote, for a
    /// member whose signature the receiver sees through a more derived alias.
    ///
    /// The parameters are converted a second time with
    /// [`PickleSupply::decl_site_erasure`] set, which makes a bare type name
    /// resolve to the abstract member the declaration was written against
    /// rather than to whatever concrete alias a subclass gives that name. The
    /// result is thrown away except for its erasure -- nothing is installed
    /// from it and no symbol it allocates is attached to a class -- so it is
    /// only ever the answer to "which method in the class file is this".
    ///
    /// A member inherited from a *generic* declaring class is converted from
    /// that class's own declaration instead (see
    /// [`PickleSupply::generic_declaration`]), since the receiver's view has
    /// already replaced the declaring class's type parameters.
    ///
    /// `None` when some parameter does not convert at all, which is the same
    /// as the caller already having failed.
    #[allow(clippy::too_many_arguments)]
    fn decl_site_want(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        class_sym: SymbolId,
        internal: &str,
        pickle_owner: &str,
        owner_module: bool,
        name: &str,
        scope: &HashMap<String, Type>,
        shape: &Shape,
    ) -> Option<Vec<Option<String>>> {
        let declared = self.generic_declaration(
            st,
            bin,
            class_sym,
            internal,
            pickle_owner,
            owner_module,
            name,
            scope,
            shape,
        );
        let seen = self.decl_site_params(st, bin, scope, shape);
        // A declaration that mentions something only the receiver's view can
        // name (an enclosing class's parameter) falls back to that view.
        let Some(declared) =
            declared.and_then(|(scope, shape)| self.decl_site_params(st, bin, &scope, &shape))
        else {
            return seen.map(|tys| tys.iter().map(|t| erased_param_desc(st, t)).collect());
        };
        let declared: Vec<Option<String>> =
            declared.iter().map(|t| erased_param_desc(st, t)).collect();
        let Some(seen) = seen else {
            return Some(declared);
        };
        // `class IntBox extends Ops[Int]` sees `first(x: Int, y: Int)`, whose
        // declaration takes two references. Erasure adapts an argument to the
        // installed member's parameter type, so a member installed with the
        // receiver's `Int` would pass a bare `int` to `(Object, Object)`
        // (VerifyError). Such a member stays with the declaring class, whose
        // own `A` is what makes erasure box the argument.
        let passes_unboxed = |t: &Type| match t {
            Type::Boolean
            | Type::Byte
            | Type::Short
            | Type::Char
            | Type::Int
            | Type::Long
            | Type::Float
            | Type::Double => true,
            Type::Class { sym, .. } => st.is_value_class(*sym),
            _ => false,
        };
        let seen_want: Vec<Option<String>> =
            seen.iter().map(|t| erased_param_desc(st, t)).collect();
        if seen
            .iter()
            .zip(&seen_want)
            .zip(&declared)
            .any(|((t, seen), declared)| seen != declared && passes_unboxed(t))
        {
            return Some(seen_want);
        }
        Some(declared)
    }

    /// [`PickleSupply::decl_site_want`] for one reading of the parameters:
    /// their types, converted at the declaration site.
    fn decl_site_params(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        scope: &HashMap<String, Type>,
        shape: &Shape,
    ) -> Option<Vec<Type>> {
        let was = std::mem::replace(&mut self.decl_site_erasure, true);
        let mut want = Vec::new();
        let mut ok = true;
        for clause in &shape.clauses {
            for p in &clause.params {
                match self.conv(st, bin, scope, &p.ty) {
                    Some(mut t) => {
                        if p.by_name && !matches!(t, Type::ByName(_)) {
                            t = Type::ByName(Box::new(t));
                        }
                        want.push(t);
                    }
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            if !ok {
                break;
            }
        }
        self.decl_site_erasure = was;
        ok.then_some(want)
    }

    /// The declaring class's own shape of an inherited member, with a scope
    /// in which that class's type parameters stand for themselves.
    ///
    /// `SigCache::lookup` hands a member back already substituted into the
    /// receiver's vocabulary: `trait Ops[A] { def first(x: A, y: A): A }`
    /// seen from `class StrBox extends Ops[String]` reads `first(x: String,
    /// y: String)`. That is the type a caller must satisfy, but the class
    /// file only has `Ops.first(Object, Object)` and the erased mixin
    /// forwarder `StrBox.first(Object, Object)` -- the JVM erases `A` at the
    /// declaration, whatever a subclass binds it to. Asking for `(String,
    /// String)` found neither, the member was never supplied, and the call
    /// fell back to the forwarder's erased `Object` result ("found: Any
    /// required: String"). A one-parameter `first(x: A)` happened to survive
    /// through `Ops`'s own completion; two parameters did not.
    ///
    /// So the raw declaration is recovered from the declaring pickle: the
    /// one member of that name whose substitution (the receiver's
    /// linearization step for the owner) is exactly the shape being
    /// installed. Its type parameters get fresh symbols, which erase as any
    /// type parameter does (an unnamed reference slot, or a primitive bound).
    /// Only the erasure of what is converted here is used; see
    /// [`PickleSupply::decl_site_want`].
    ///
    /// `None` for a member declared on the receiver itself, a declaring class
    /// with no type parameters, or when the raw declaration is not uniquely
    /// identified -- the receiver's view is then all there is.
    #[allow(clippy::too_many_arguments)]
    fn generic_declaration(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        class_sym: SymbolId,
        internal: &str,
        pickle_owner: &str,
        owner_module: bool,
        name: &str,
        scope: &HashMap<String, Type>,
        shape: &Shape,
    ) -> Option<(HashMap<String, Type>, Shape)> {
        let receiver_module = st.get(class_sym).kind == SymKind::ModuleClass;
        let receiver = self.pickled_full_name(bin, internal, receiver_module)?;
        if receiver == pickle_owner && receiver_module == owner_module {
            return None;
        }
        let sig = {
            let mut src = BinSource(bin);
            self.sigs
                .class_sig(&mut src, pickle_owner, owner_module)
                .ok()?
        };
        if sig.tparams.is_empty() {
            return None;
        }
        let subst = {
            let mut src = BinSource(bin);
            let mut errs = Vec::new();
            self.sigs
                .linearization(&mut src, &receiver, receiver_module, &mut errs)
                .into_iter()
                .find(|step| step.class_name == pickle_owner && step.module == owner_module)?
                .subst
        };
        let mut declared: Option<Shape> = None;
        for member in sig.members_named(name) {
            if member.kind != MemberKind::Def {
                continue;
            }
            let seen = read_shape(&scala_rs_pickle::sym::apply_subst(&member.ty, &subst))
                .and_then(pin_undetermined_tparams);
            if !seen.is_some_and(|seen| same_value_params(&seen, shape)) {
                continue;
            }
            // Two declarations that the receiver's binding makes alike are
            // two overloads to the caller; neither is "the" declaration.
            if declared.is_some() {
                return None;
            }
            declared = Some(read_shape(&member.ty).and_then(pin_undetermined_tparams)?);
        }
        let declared = declared?;
        let mut scope = scope.clone();
        let mut owner_tparams = Vec::new();
        for tp in &sig.tparams {
            // A method type parameter shadows the class's of the same name.
            if declared.tparams.iter().any(|m| m.name == tp.name) {
                continue;
            }
            let tp = shape_tparam(tp);
            let id = alloc_shape_tparam(st, SymbolId::NONE, &tp);
            scope.insert(tp.name.clone(), Type::TypeParam(id));
            owner_tparams.push((tp, id));
        }
        for (tp, id) in &owner_tparams {
            self.resolve_shape_tparam_bounds(st, bin, &scope, tp, *id);
        }
        Some((scope, declared))
    }

    /// JVM erases a type parameter of the declaring owner to `Object`, even
    /// when a receiver's parent substitution makes that parameter concrete.
    /// Keep this relaxation limited to an implicit parameter whose raw
    /// declaration is an application of the owner's type parameter. A direct
    /// `ClassTag[A]` declaration must continue to require a ClassTag slot.
    fn inherited_generic_evidence_slots(
        &mut self,
        bin: &mut BinaryIndex,
        internal: &str,
        pickle_owner: &str,
        owner_module: bool,
        name: &str,
        shape: &Shape,
    ) -> Option<Vec<bool>> {
        let receiver = internal.replace('/', ".");
        let owner = pickle_owner.replace('/', ".");
        if receiver.trim_end_matches('$') == owner.trim_end_matches('$') {
            return None;
        }
        if !shape.clauses.iter().any(|clause| clause.implicit) {
            return None;
        }
        let sig = {
            let mut src = BinSource(bin);
            self.sigs
                .class_sig(&mut src, pickle_owner, owner_module)
                .ok()?
        };
        let owner_tparams: HashSet<String> = sig.tparams.iter().map(|tp| tp.name.clone()).collect();
        if owner_tparams.is_empty() {
            return None;
        }
        // Do not infer correspondence from arity alone when an owner has
        // same-shaped overloads. The caller-facing member and the raw member
        // must at least have the same clause/implicit layout.
        let candidates: Vec<Shape> = sig
            .members_named(name)
            .filter(|member| member.kind == MemberKind::Def)
            .filter_map(|member| read_shape(&member.ty))
            .filter(|candidate| {
                candidate.clauses.len() == shape.clauses.len()
                    && candidate
                        .clauses
                        .iter()
                        .zip(&shape.clauses)
                        .all(|(raw, seen)| {
                            raw.implicit == seen.implicit && raw.params.len() == seen.params.len()
                        })
            })
            .collect();
        let candidate = match candidates.as_slice() {
            [candidate] => candidate,
            _ => return None,
        };
        let mut slots = Vec::with_capacity(shape.arity());
        let mut found = false;
        for clause in &candidate.clauses {
            for param in &clause.params {
                let evidence = clause.implicit
                    && sig_type_is_owner_tparam_application(&param.ty, &owner_tparams);
                slots.push(evidence);
                found |= evidence;
            }
        }
        found.then_some(slots)
    }

    /// `ty` seen as an instance of its base class `owner`, with the
    /// arguments `ty`'s parent clauses pass up to it; `ty` itself when its
    /// class is `owner`, and `None` when `owner` is not among its bases or
    /// has no type parameters to read.
    ///
    /// A classpath class gets its pickled parents only on demand (see
    /// [`PickleSupply::ensure_parents`]), and one class at a time, so the
    /// ancestry is completed here before it is walked.
    fn base_type_at(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        ty: &Type,
        owner: SymbolId,
    ) -> Option<Type> {
        let cls = st.class_sym_of(ty)?;
        if cls == owner {
            return Some(ty.clone());
        }
        // Only a class type has parent clauses to read the owner's arguments
        // off, and only a generic owner has arguments to read. A plain one
        // gains nothing, and taking it anyway was not harmless: attaching
        // `JavaUniverse` seen as the non-generic `Exprs` to
        // `scala.tools.reflect.Eval(expr: JavaUniverse#Expr[T])` made
        // `reify { v }` come out as `JavaUniverse.this.Expr[A]` rather than
        // `universe.Expr[A]`.
        if !matches!(ty, Type::Class { .. }) || st.get(owner).tparams.is_empty() {
            return None;
        }
        let mut pending = vec![cls];
        let mut seen = HashSet::new();
        while let Some(c) = pending.pop() {
            if c == owner || !seen.insert(c) {
                continue;
            }
            self.ensure_parents(st, bin, c);
            pending.extend(st.get(c).parents.iter().filter_map(|p| st.class_sym_of(p)));
        }
        st.base_type_seq(ty)
            .into_iter()
            .find(|base| st.class_sym_of(base) == Some(owner))
    }

    /// Whether `anc` is a strict ancestor of `cls`, asked of the *pickle*.
    ///
    /// The symbol table cannot answer it: `scala.collection.MapOps` has no
    /// class symbol at all in a program that never names it, so
    /// `find_class_by_jvm` returns nothing and every pair reads as unrelated.
    /// The pickle always has it, because that is where the member was just
    /// read from.
    fn declares_above(&mut self, bin: &mut BinaryIndex, anc: &str, cls: &str) -> bool {
        if anc == cls {
            return false;
        }
        let mut src = BinSource(bin);
        let mut errs = Vec::new();
        self.sigs
            .linearization(&mut src, cls, false, &mut errs)
            .iter()
            .any(|s| s.class_name == anc)
    }

    /// Resolve a pickled method type parameter's bounds in the vocabulary of
    /// both its enclosing method and its own higher-kinded binders.
    fn resolve_shape_tparam_bounds(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        outer_scope: &HashMap<String, Type>,
        shape: &ShapeTParam,
        id: SymbolId,
    ) {
        let mut scope = outer_scope.clone();
        let inner_ids = st.get(id).tparams.clone();
        for (inner, inner_id) in shape.inner.iter().zip(inner_ids.iter().copied()) {
            scope.insert(inner.name.clone(), Type::TypeParam(inner_id));
        }
        for (inner, inner_id) in shape.inner.iter().zip(inner_ids.iter().copied()) {
            self.resolve_shape_tparam_bounds(st, bin, &scope, inner, inner_id);
        }
        if let Some(hi) = &shape.hi {
            if let Some(t) = self.conv(st, bin, &scope, hi) {
                st.get_mut(id).bound_hi = Some(t);
            }
        }
        if let Some(lo) = &shape.lo {
            if !matches!(lo, SigType::Ref { sym, .. } if sym == "scala.Nothing") {
                if let Some(t) = self.conv(st, bin, &scope, lo) {
                    st.get_mut(id).bound_lo = Some(t);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn install(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        class_sym: SymbolId,
        internal: &str,
        name: &str,
        jvm_member: &str,
        // The pickled class that *declares* this member, which is not
        // `internal` whenever the member is inherited.
        pickle_owner: &str,
        owner_module: bool,
        shape: &Shape,
        class_scope: &HashMap<String, Type>,
        result_prefix: Option<&SigType>,
        seen_shapes: &mut Vec<(SymbolId, String, usize, Vec<Type>)>,
        // Members detached again because a later declaration with the same
        // explicit parameters and an extra implicit clause took their
        // place; the caller must not hand them back either.
        superseded: &mut Vec<SymbolId>,
    ) -> Option<SymbolId> {
        // Allocated ownerless, so a failure leaves nothing behind:
        // `SymbolTable::alloc` pushes into the owner's member list.
        let m = st.alloc(name, SymbolId::NONE, SymKind::Method, Flags::EMPTY, "");

        let mut scope = class_scope.clone();
        let mut tparams = Vec::new();
        for tp in &shape.tparams {
            let id = alloc_shape_tparam(st, m, tp);
            scope.insert(tp.name.clone(), Type::TypeParam(id));
            tparams.push(id);
        }
        // Bounds are resolved after every parameter is in scope (`A <: B`).
        // The lower bound matters as much as the upper one: `[B >: A]` is what
        // lets the typer solve `B` from the receiver for `xs.reduceOption`.
        for (tp, id) in shape.tparams.iter().zip(tparams.iter().copied()) {
            self.resolve_shape_tparam_bounds(st, bin, &scope, tp, id);
        }

        let mut paramss_ty: Vec<Vec<Type>> = Vec::new();
        let mut paramss_sym: Vec<Vec<SymbolId>> = Vec::new();
        // 1-based positions of parameters the caller may omit.
        let mut default_slots: Vec<usize> = Vec::new();
        for clause in &shape.clauses {
            let mut tys = Vec::new();
            let mut syms = Vec::new();
            for p in &clause.params {
                // Later clauses may mention an earlier value parameter's
                // singleton type, as BuildFrom[xs.type, A, C] does. Retain
                // its identity so application substitutes the actual xs.
                let saved_singletons = std::mem::take(&mut self.param_singletons);
                let saved_symbols = std::mem::take(&mut self.param_singleton_symbols);
                for (prior, symbols) in shape.clauses.iter().zip(&paramss_sym) {
                    for (parameter, &symbol) in prior.params.iter().zip(symbols) {
                        let key = format!("{pickle_owner}.{name}.{}", parameter.name);
                        self.param_singletons.insert(
                            key.clone(),
                            Type::SingleType {
                                prefix: Box::new(Type::NoType),
                                sym: symbol,
                            },
                        );
                        self.param_singleton_symbols.insert(key, symbol);
                    }
                }
                let converted = self.conv(st, bin, &scope, &p.ty);
                self.param_singletons = saved_singletons;
                self.param_singleton_symbols = saved_symbols;
                let Some(mut t) = converted else {
                    trace(format_args!(
                        "{internal}#{name}: parameter {} has an unmappable type {:?}",
                        p.name, p.ty
                    ));
                    return None;
                };
                if p.by_name && !matches!(t, Type::ByName(_)) {
                    t = Type::ByName(Box::new(t));
                }
                let mut flags = if clause.implicit {
                    Flags::PARAM.with(Flags::IMPLICIT)
                } else {
                    Flags::PARAM
                };
                if p.flags & pflags::DEFAULTPARAM != 0 {
                    flags = flags.with(Flags::DEFAULTPARAM);
                    default_slots
                        .push(paramss_ty.iter().map(|c| c.len()).sum::<usize>() + tys.len() + 1);
                }
                let ps = st.alloc(
                    scala_rs_pickle::names::decode_method_name(&p.name),
                    m,
                    SymKind::Term,
                    flags,
                    "",
                );
                st.get_mut(ps).ty = t.clone();
                tys.push(t);
                syms.push(ps);
            }
            paramss_ty.push(tys);
            paramss_sym.push(syms);
        }
        // `def apply[F[_]](implicit F: Async[F]): F.type = F` -- how every
        // cats-effect type class writes its summoner. The result is a
        // singleton of the method's own parameter, which nothing outside this
        // signature can name; without a reading for it the whole member was
        // declined and `Async$.apply` stayed as the class file's erased
        // `apply(Async): Async`, whose parameter carries no `implicit` flag.
        // `Async[F]` was then a bare method type and `Async[F].pure(a)` read
        // "value pure is not a member of (Async[F])Async[F]".
        //
        // The parameter's declared type is that singleton's widening, and it
        // has exactly the members a selection off the result can reach, which
        // is all a summoner is for.
        let saved_singletons = std::mem::take(&mut self.param_singletons);
        let saved_singleton_symbols = std::mem::take(&mut self.param_singleton_symbols);
        for ((clause, tys), syms) in shape
            .clauses
            .iter()
            .zip(paramss_ty.iter())
            .zip(paramss_sym.iter())
        {
            for ((p, t), ps) in clause.params.iter().zip(tys.iter()).zip(syms) {
                let key = format!("{pickle_owner}.{name}.{}", p.name);
                self.param_singletons.insert(key.clone(), t.clone());
                self.param_singleton_symbols.insert(key, *ps);
            }
        }
        // `def get[A](a: A): Outer.this.Repr[A]` in a nested class pickles the
        // member bare, and `self_type_member` would read `Repr` on the nested
        // class, where a same-named alias (`type Repr[A] = Vector[A]`) is a
        // different type. The `This` prefix names the class to read it on.
        let enclosing_self = match (result_prefix, &shape.ret) {
            (Some(SigType::This(this_owner)), SigType::Ref { sym, .. })
                if this_owner != pickle_owner
                    && !sym.contains('.')
                    && !scope.contains_key(sym) =>
            {
                self.ensure_class(st, bin, this_owner, false)
                    .map(|cls| Type::Class {
                        sym: cls,
                        args: st
                            .get(cls)
                            .tparams
                            .iter()
                            .map(|t| Type::TypeParam(*t))
                            .collect(),
                    })
            }
            _ => None,
        };
        let saved_self = enclosing_self.map(|t| self.self_ty.replace(t));
        let ret = self.conv(st, bin, &scope, &shape.ret);
        if let Some(saved_self) = saved_self {
            self.self_ty = saved_self;
        }
        self.param_singletons = saved_singletons;
        self.param_singleton_symbols = saved_singleton_symbols;
        let Some(mut ret) = ret else {
            trace(format_args!(
                "{internal}#{name}: unmappable result type {:?}",
                shape.ret
            ));
            return None;
        };
        // Only the declaring class's own `this`: member selection re-reads the
        // name on the receiver's class, and an enclosing class's
        // `Outer.this.Repr[A]` would otherwise pick up a same-named member of
        // the inner receiver instead.
        //
        // A member declared on the class itself is pickled bare (`Repr`); one
        // inherited from a base trait is pickled under its owner
        // (`lib.Ops.Repr` for `Mixed.this.Repr` when `Ops` declares `Repr`).
        // Either way it is `this`'s type member, which the receiver reads by
        // its simple name. A dotted name counts only when it converted to an
        // abstract type member, so an inner class spelled the same way does
        // not.
        let result_type_member_app = match (result_prefix, &shape.ret) {
            (Some(SigType::This(this_owner)), SigType::Ref { sym, args })
                if this_owner == pickle_owner && !scope.contains_key(sym) =>
            {
                let member = match sym.rsplit_once('.') {
                    None => Some(sym.as_str()),
                    Some((_, member)) if is_type_member_result(&ret) => Some(member),
                    Some(_) => None,
                };
                member.and_then(|member| {
                    self.conv_all(st, bin, &scope, args, 0)
                        .map(|args| (member.to_string(), args))
                })
            }
            _ => None,
        };
        // An inner class may be returned through an applied outer class rather
        // than through `this`: `def ops[T](x: T)(implicit ord: Ordering[T]):
        // Ordering[T]#OrderingOps`. The compact signature stores the result's
        // class separately from its TypeRef prefix. Keep the applied outer
        // type on the result so a later selection can substitute the outer
        // class's parameters in members of the inner class.
        //
        // The prefix may also be a *subclass* of the class declaring the
        // inner one: `def mk[U](s: Sub[U]): Sub[U]#Inner` where `class
        // Sub[U] extends Outer[List[U]]` and `Inner` is `Outer`'s. `Inner`'s
        // members are written in `Outer`'s parameters, so what they are read
        // at is `Sub[U]` seen as an `Outer` -- `Outer[List[U]]` -- and not
        // `Sub[U]` itself, whose `U` is a different parameter altogether.
        // Requiring the prefix's class to *be* the owner attached nothing
        // here, and `mk(s).get` came out as the bare `T`.
        if let (Some(prefix @ SigType::Ref { .. }), Type::Class { sym, .. }) =
            (result_prefix, crate::prefix::strip_view(&ret))
        {
            let sym = *sym;
            if crate::prefix::view_prefix(&ret).is_none() {
                if let Some(outer) = self.conv_at(st, bin, &scope, prefix, 0) {
                    let owner = st.get(sym).owner;
                    if !owner.is_none()
                        && st.get(sym).kind == SymKind::Class
                        && st.get(owner).kind == SymKind::Class
                    {
                        if let Some(outer) = self.base_type_at(st, bin, &outer, owner) {
                            ret = crate::prefix::with_prefix(ret, outer);
                        }
                    }
                }
            }
        }
        // The erased descriptor comes from the classfile itself rather than
        // from re-deriving scalac's erasure: the bytes are the truth, and a
        // descriptor we merely guessed would fail to link. Resolved now that
        // the parameters are known, so same-arity overloads
        // (`Iterator.from(Int)` vs `from(IterableOnce)`) can be told apart.
        let want: Vec<Option<String>> = paramss_ty
            .iter()
            .flatten()
            .map(|t| erased_param_desc(st, t))
            .collect();
        // `pickled_origin` only recognises a duplicate that was itself
        // installed from a pickle -- it deliberately says nothing about a
        // hand-written prelude member, so two copies of the same pickled
        // declaration reaching the same receiver still collapse
        // (`collapse_pickled_copies`) while a genuine prelude override is
        // never treated as a spurious duplicate of a pickle hit. But a
        // *pickle* copy of a member the *prelude itself already declared on
        // this exact class* slips past both checks: it shares the prelude
        // symbol's owner (so `drop_overridden`'s override rule, which only
        // fires across owners, does not apply) and it is the only one of the
        // pair carrying a `pickled_origin` (so `collapse_pickled_copies`,
        // which only merges when *both* sides have one, does not apply
        // either). `object Set extends IterableFactory[Set]`'s `apply` is
        // hand-written in `prelude_coll` (`add_set`) so `Set(1, 2)` still
        // works without the jar; asking the companion for `apply` again
        // after something had already forced a *different* member
        // (`SetOps.apply(A): Boolean`, from `u("x")`) to complete re-read
        // `apply` from the jar and installed a second, pickle-derived copy
        // of the very same overload next to it -- and a later `Set("admin")`
        // saw both and was `ambiguous overload`, depending on whether
        // something upstream had already touched the class. This is that
        // case: prelude always wins, so a same-shaped hand-written member
        // already on `class_sym` means nothing new is installed for it --
        // see below for what is reported instead.
        //
        // `s.0 < st.prelude_end` is the part that must not be dropped: an
        // empty `pickled_origin` is *also* what a member the raw classfile
        // reader put there carries (`adopt_binary_class`'s "stale" members,
        // which that function's own loop means to replace with the richer
        // pickled signature this call is about to install). Those symbols
        // are allocated long after the static prelude is built, so their id
        // is never below `prelude_end`. Without this half of the condition,
        // `scala.Equals.canEqual` and friends never got their pickle-precise
        // signature (`adopt_binary_class` saw `installed.is_empty()` and
        // left the crude classfile one in place), and every case class
        // failed its `Equals` override check with "needs to be abstract".
        //
        // Handing back `None` here -- as if the hit simply could not be
        // installed -- hid the prelude member from every caller that reads
        // `complete_named`'s *return value*, not just from `class_sym`'s own
        // member list (which had it all along). `PickleSupply::complete`'s
        // class-plus-companion union (`agent/oshadow`, `agent/companionkind`)
        // builds its answer purely from what completion reports back, never
        // re-running `lookup_member`; a bare decline made
        // `scala.math.BigDecimal`'s prelude-written `apply(Int)` /
        // `apply(String)` / `apply(java.math.BigDecimal)` invisible to that
        // union the moment something else forced `apply` to complete, and
        // the overload set `type_select` built -- and `record_overload_group`
        // then cached under the callee symbol -- had every *other* `apply`
        // shape but not those three. `BigDecimal(2)` then had no exact match
        // among `Long` / `Double` / `BigInt` and came out `ambiguous overload`
        // before `Check::widen_with_companion` (which adds the missing three
        // back in) ever ran: that fallback only fires when a resolution
        // reports no match at all, and an *ambiguous* one reports and keeps
        // a diagnostic straight away. Reporting the existing symbol instead
        // keeps every caller's count and `found` set exactly as if the
        // prelude member had always been part of the pickle's own answer,
        // while still installing nothing new and leaving `class_sym.members`
        // untouched.
        if let Some(&blocker) = st.get(class_sym).members.iter().find(|&&s| {
            s.0 < st.prelude_end && {
                let e = st.get(s);
                e.name == name
                    && e.pickled_origin.is_empty()
                    && flat_erased_params(st, &e.ty) == want
            }
        }) {
            trace(format_args!(
                "{internal}#{name}: a hand-written prelude overload already has these \
                 erased parameters -- reporting it instead of installing a pickle copy"
            ));
            return Some(blocker);
        }
        // Lambda arguments can be pretyped from the common input types of
        // several alternatives. Preserve those alternatives so their result
        // types can distinguish scalar and pair-producing collection calls.
        let arity: usize = paramss_ty.iter().map(|c| c.len()).sum();
        // The shape two alternatives are compared by is their *explicit*
        // parameters: nsc's `isAsSpecific` looks through an implicit clause
        // (`case mt: MethodType if mt.isImplicit => isAsSpecific(restpe, …)`),
        // so `SortedSetOps.collect(pf)(implicit Ordering[B])` and
        // `IterableOps.collect(pf)` are equally specific and only their owners
        // separate them -- and the owner is exactly what pulling both copies
        // down onto the receiver throws away. Keyed on the full list they
        // looked like two different overloads, both survived, and every
        // `TreeSet.collect(pf)` / `.map(f)` was `ambiguous overload`.
        // Linearization order keeps the more derived one, which is the one
        // whose `Ordering` witness makes the result a `TreeSet`.
        let explicit_types: Vec<Type> = shape
            .clauses
            .iter()
            .zip(&paramss_ty)
            .filter(|(clause, _)| !clause.implicit)
            .flat_map(|(_, tys)| tys.iter().cloned())
            .collect();
        // Compare method variables positionally, preserving the structure
        // around them. IterableOnce[B] and IterableOnce[(K, V2)] are distinct
        // overloads even though both have one variable and the same erasure.
        let same_parameters = |kept: SymbolId, previous: &[Type]| {
            let old_tparams = &st.get(kept).tparams;
            old_tparams.len() == tparams.len()
                && previous.len() == explicit_types.len()
                && previous.iter().zip(&explicit_types).all(|(old, new)| {
                    let renamed = crate::symbol::subst_tparams_slice(
                        old_tparams,
                        &tparams
                            .iter()
                            .copied()
                            .map(Type::TypeParam)
                            .collect::<Vec<_>>(),
                        old,
                    );
                    renamed == *new
                })
        };
        // A derived declaration can add an implicit clause to the same
        // explicit parameters, as SortedMapOps does with Ordering. Preserve
        // that more specific declaration regardless of traversal order.
        let mut supersedes: Option<SymbolId> = None;
        if let Some((kept, kept_owner, kept_arity)) = seen_shapes
            .iter()
            .find(|(k, _, _, ps)| same_parameters(*k, ps))
            .map(|(k, o, a, _)| (*k, o.clone(), *a))
        {
            if arity > kept_arity
                && shape.clauses.iter().any(|c| c.implicit)
                && self.declares_above(bin, &kept_owner, pickle_owner)
            {
                trace(format_args!(
                    "{internal}#{name}: {pickle_owner} adds an implicit clause to \
                     {kept_owner}'s declaration, so it supersedes it"
                ));
                supersedes = Some(kept);
            } else {
                trace(format_args!(
                    "{internal}#{name}: skipping an overload shadowed by a more \
                     derived declaration with the same parameters"
                ));
                return None;
            }
        }
        // Resolved before the shape is claimed: an alternative that has no
        // descriptor is not supplied, so it must not shadow the next one
        // either (`TreeMap.collect(pf)` erases to two class-file methods and
        // would otherwise have taken `collect(pf)(Ordering)`'s place).
        let owner_file = scala_rs_pickle::sym::pickle_files_for(pickle_owner, owner_module)
            .into_iter()
            .find(|file| bin.find_class(file).ok().flatten().is_some());
        let decl_params = self
            .decl_site_want(
                st,
                bin,
                class_sym,
                internal,
                pickle_owner,
                owner_module,
                name,
                &scope,
                shape,
            )
            .unwrap_or_else(|| want.clone());
        let declared = owner_file.as_ref().and_then(|owner| {
            self.erased_desc_return(
                bin,
                owner,
                jvm_member,
                &decl_params,
                erased_return_desc(st, &ret).as_deref(),
            )
        });
        let declared_generic_evidence = owner_file.as_ref().and_then(|owner| {
            if declared.is_some() {
                return None;
            }
            let slots = self.inherited_generic_evidence_slots(
                bin,
                internal,
                pickle_owner,
                owner_module,
                name,
                shape,
            )?;
            if slots.len() != decl_params.len() {
                return None;
            }
            let mut relaxed = decl_params.clone();
            for (param, evidence) in relaxed.iter_mut().zip(slots) {
                if evidence {
                    *param = None;
                }
            }
            if relaxed == decl_params {
                return None;
            }
            trace(format_args!(
                "{internal}#{name}: retrying declaration descriptor with generic evidence slots as unknown references"
            ));
            self.erased_desc_return(
                bin,
                owner,
                jvm_member,
                &relaxed,
                erased_return_desc(st, &ret).as_deref(),
            )
        });
        let found = match declared
            .or(declared_generic_evidence)
            .or_else(|| self.erased_desc(bin, internal, jvm_member, &want))
        {
            Some(found) => Some(found),
            // The signature and the descriptor are erased in *different*
            // vocabularies when a more derived class in the linearisation
            // makes an abstract type member concrete. `Mirrors.RuntimeMirror`
            // declares `def classSymbol(rtcls: RuntimeClass): ClassSymbol`
            // against `type RuntimeClass >: Null <: AnyRef`, so its class file
            // says `classSymbol(Ljava/lang/Object;)`; a `JavaUniverse`'s
            // mirror sees the same member with `type RuntimeClass =
            // java.lang.Class[_]`, which is the type a caller must satisfy and
            // erases to `Ljava/lang/Class;`. Wanting the caller's erasure
            // found no method at all and the member was dropped, so
            // `runtimeMirror(cl).classSymbol(classOf[A])` was "no matching
            // overload for (Mirrors.RuntimeClass)Symbols.ClassSymbol".
            //
            // So: keep the *caller's* view as the member's type, and go back
            // to the declaration's view (`decl_params`, above) only to find
            // the bytes to call, searched from the receiver this time. Skipped
            // outright when the two views agree -- which they do for every
            // member whose signature names no such alias.
            None if decl_params != want => {
                self.erased_desc(bin, internal, jvm_member, &decl_params)
            }
            None => None,
        };
        let Some(found) = found else {
            trace(format_args!(
                "{internal}#{name}/{}: no unambiguous erased descriptor (want {want:?})",
                shape.arity()
            ));
            return None;
        };
        seen_shapes.push((m, pickle_owner.to_string(), arity, explicit_types));
        // Lookup installs the method on the receiver, but invocation must
        // name the actual JVM declaration together with its descriptor.
        // An inherited overload can have the same erased arguments and a
        // different erased return type from a receiver-local declaration.
        if found.off_the_bytecode_path || found.declared_in != internal {
            trace(format_args!(
                "{internal}#{name}: declared by {}, which the receiver's class file \
                 does not reach -- the call will name it",
                found.declared_in
            ));
            st.get_mut(m).declaring_class = found.declared_in;
            st.get_mut(m).declaring_is_interface = found.declared_by_interface;
        }

        // Which pickled declaration this copy stands for. The receiver's class
        // is *not* part of it: the point is that the same declaration, pulled
        // down onto two different classes, is recognisable as one member.
        // Source overload identity cannot be derived from the JVM descriptor:
        // two Scala parameter types may erase to the same descriptor while
        // remaining distinct alternatives (for example a primitive and a
        // value-class wrapper around it). Use the pickled declaration shape,
        // which is also stable when the same inherited member is installed on
        // more than one receiver.
        st.get_mut(m).pickled_origin = format!("{pickle_owner}#{jvm_member}{shape:?}");
        let mut source = BinSource(bin);
        let mut errors = Vec::new();
        st.get_mut(m).pickled_owner_bases = self
            .sigs
            .linearization(&mut source, pickle_owner, owner_module, &mut errors)
            .into_iter()
            .map(|base| base.class_name)
            .collect();
        st.set_jvm_name(m, found.desc);
        st.get_mut(m).tparams = tparams;
        st.get_mut(m).params = paramss_sym.iter().flatten().copied().collect();
        st.get_mut(m).paramss = paramss_sym;
        st.get_mut(m).ty = Type::Method {
            paramss: paramss_ty,
            ret: Box::new(ret),
        };
        // A parameter the caller may omit is filled from the class's
        // `<method>$default$<n>` getter. Without it the typer fills nothing and
        // the call goes out with fewer arguments than the descriptor declares,
        // which the verifier rejects -- so a getter we cannot supply makes the
        // whole member ineligible. Done before attaching `m`, so declining
        // leaves the class untouched.
        for slot in default_slots {
            let getter = format!("{name}$default${slot}");
            let ids = self.complete_named(st, bin, class_sym, &getter, true);
            let Some(&gid) = ids.first() else {
                trace(format_args!(
                    "{internal}#{name}: no {getter}, so the default cannot be filled"
                ));
                return None;
            };
            // `default_getter_apply` passes the arguments that precede the
            // defaulted one, truncated to the getter's own arity.  scalac's
            // getter includes a prefix of those arguments: a nullary getter
            // is common when the default does not read an earlier parameter
            // (`SeqOps.lastIndexOf$default$2()` takes nothing though `elem`
            // comes first), and a later clause can omit the preceding
            // parameters from its own clause (`AbstractTable.foreignKey$default$5`
            // takes the three arguments from the first clause, not the
            // `targetColumns` argument that precedes `onUpdate` in source).
            // Any arity up to the global prefix is therefore linkable; the
            // caller truncates to exactly this count below.
            let want_args = slot - 1;
            let got_args = st.get(gid).params.len();
            if got_args > want_args {
                trace(format_args!(
                    "{internal}#{name}: {getter} takes {got_args} argument(s), which is \
                     more than the {want_args} arguments that precede the default"
                ));
                return None;
            }
        }

        if shape.implicit {
            let f = st.get(m).flags.with(Flags::IMPLICIT);
            st.get_mut(m).flags = f;
        }
        // Done only now that the derived declaration is known to install: an
        // alternative with no usable descriptor must not take the place of the
        // one already in.
        if let Some(old) = supersedes {
            st.get_mut(class_sym).members.retain(|&x| x != old);
            seen_shapes.retain(|(k, _, _, _)| *k != old);
            superseded.push(old);
        }
        st.get_mut(m).owner = class_sym;
        st.get_mut(class_sym).members.push(m);
        if let Some(app) = result_type_member_app {
            self.result_type_member_apps.insert(m, app);
        }
        Some(m)
    }

    /// [`PickleSupply::attach_parents`] for a class nobody has asked a member
    /// of, working out its pickle name itself.
    ///
    /// Completing any member attaches the parents on the way
    /// (`complete_named`), so a class the typer has already *used* has them.
    /// One the typer has only *named* does not, and `import <a value>._` is
    /// exactly that case: what the wildcard offers is almost all inherited,
    /// and deciding what the value even is (`Check::is_reflect_universe`: does
    /// this extend `scala.reflect.api.Universe`?) reads the same parent list.
    /// `IterableOps[A, CC, C]` as the pickle of the class `internal`
    /// instantiates it along its linearization: the dotted names of the
    /// classes `CC` and `C` stand for, when both are plain class references.
    /// `NumericRange[T]` answers `IndexedSeq` twice, `IndexedSeqView[A]`
    /// `View` twice, `Vector[A]` itself. See `crate::ops_shape`.
    pub(crate) fn iterable_ops_classes(
        &mut self,
        bin: &mut BinaryIndex,
        internal: &str,
    ) -> Option<(String, String)> {
        let full = self.pickled_full_name(bin, internal, false)?;
        let mut errs = Vec::new();
        let lin = {
            let mut src = BinSource(bin);
            self.sigs.linearization(&mut src, &full, false, &mut errs)
        };
        let step = lin
            .iter()
            .find(|s| !s.module && s.class_name == "scala.collection.IterableOps")?;
        fn head(t: &SigType) -> Option<String> {
            match t {
                SigType::Annotated(t) => head(t),
                SigType::Ref { sym, .. } if sym.contains('.') => Some(sym.clone()),
                _ => None,
            }
        }
        Some((head(step.subst.get("CC")?)?, head(step.subst.get("C")?)?))
    }

    /// The value-parameter counts (all clauses together) of every `name` the
    /// pickled linearization of the class `internal` declares -- enough to
    /// tell `SortedSetOps.map(f)(implicit ord)` from `IterableOps.map(f)`
    /// when the class files cannot (the `Ops` trait declares it, and the
    /// receiver's own interface file does not).
    ///
    /// Each count comes with the dotted name of the class declaring it, most
    /// derived first.
    pub(crate) fn pickled_arities(
        &mut self,
        bin: &mut BinaryIndex,
        internal: &str,
        name: &str,
    ) -> Vec<(usize, String)> {
        let Some(full) = self.pickled_full_name(bin, internal, false) else {
            return Vec::new();
        };
        let enc = scala_rs_pickle::names::encode_method_name(name);
        let (hits, _errs) = {
            let mut src = BinSource(bin);
            self.sigs.lookup(&mut src, &full, false, &enc)
        };
        fn count(t: &SigType) -> usize {
            match t {
                SigType::Poly { result, .. } => count(result),
                SigType::Method { params, result, .. } => params.len() + count(result),
                _ => 0,
            }
        }
        hits.iter()
            .filter(|h| h.member.kind == MemberKind::Def && !h.owner_module)
            .map(|h| (count(&h.member.ty), h.owner.clone()))
            .collect()
    }

    pub fn ensure_parents(&mut self, st: &mut SymbolTable, bin: &mut BinaryIndex, cls: SymbolId) {
        if cls.is_none() || self.parented.contains(&cls.0) {
            return;
        }
        let sym = st.get(cls);
        if !sym.is_class_like() {
            return;
        }
        let internal = sym.jvm_name.clone();
        // The library, a class already adopted, or any other class off a jar
        // that still has a pickle to read. The last case is what an implicit
        // *candidate* needs: `warm_implicit_candidates` reaches a class the
        // program merely named, and nothing has completed a member of it, so
        // requiring `adopted` here meant the parent list stayed whatever the
        // class file's `Signature` attribute said -- which for a class over a
        // `@specialized` one is a specialized subclass with the parameter
        // already fixed to `Object`. `pickled_full_name` returns `None` when
        // there is no pickle, so a plain Java class still keeps its own path.
        if !internal.starts_with("scala/")
            && !self.adopted.contains(&cls.0)
            && (cls.0 < st.prelude_end
                || internal.starts_with("java/")
                || internal.starts_with("javax/"))
        {
            return;
        }
        let is_module = sym.kind == SymKind::ModuleClass;
        let Some(full) = self.pickled_full_name(bin, &internal, is_module) else {
            return;
        };
        self.attach_parents(st, bin, cls, &full, is_module);
    }

    /// [`Self::ensure_parents`] for `cls` and, transitively, every class it
    /// inherits from, so [`SymbolTable::is_ancestor_of`] sees the whole chain.
    fn ensure_ancestor_parents(&mut self, st: &mut SymbolTable, bin: &mut BinaryIndex, cls: SymbolId) {
        let mut stack = vec![cls];
        let mut seen = rustc_hash::FxHashSet::default();
        while let Some(c) = stack.pop() {
            if !seen.insert(c.0) {
                continue;
            }
            self.ensure_parents(st, bin, c);
            for parent in st.get(c).parents.clone() {
                if let Some(p) = st.class_sym_of(&parent) {
                    stack.push(p);
                }
            }
        }
    }

    /// Give a class the parents its own pickle declares, if it does not have
    /// them already.
    ///
    /// See [`ensure_value_class_field`] for the other half of recognising a
    /// value class that arrives on `-cp`.
    ///
    /// The prelude declares `immutable.Set` without `collection.Set` above it,
    /// so `Set#&`, whose parameter is `collection.Set[A]`, could be supplied
    /// but never called. Attaching the pickled parents closes that gap, and it
    /// is additive: an existing parent is never removed or replaced, and this
    /// only runs on classes a lookup already failed on.
    fn attach_parents(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        class_sym: SymbolId,
        full: &str,
        is_module: bool,
    ) {
        if !self.parented.insert(class_sym.0) {
            return;
        }
        let Ok(sig) = ({
            let mut src = BinSource(bin);
            self.sigs.class_sig(&mut src, full, is_module)
        }) else {
            return;
        };
        // Parent completion can be the first operation on a classpath class.
        // Its pickle types use the class's own parameters, so install those
        // identities before converting either parents or a value class's
        // wrapped constructor field.
        adopt_tparam_kinds(st, class_sym, &sig);
        let mut scope: HashMap<String, Type> = HashMap::new();
        for tp in &st.get(class_sym).tparams {
            scope.insert(st.get(*tp).name.clone(), Type::TypeParam(*tp));
        }
        // A JVM Signature erases the bound on `CC[_]`; the Scala pickle can
        // retain a bound such as `CC[X] <: IterableOps[X, CC, CC[X]]`. Without
        // it, selecting a member on a method result of type `CC[A]` sees an
        // unconstrained type constructor even though the class declaration
        // guarantees the collection operations.
        let class_tparams = st.get(class_sym).tparams.clone();
        if sig.tparams.len() == class_tparams.len() {
            for (tp, id) in sig.tparams.iter().zip(class_tparams) {
                if !st.get(id).tparams.is_empty() && st.get(id).bound_hi.is_none() {
                    let shape = shape_tparam(tp);
                    self.resolve_shape_tparam_bounds(st, bin, &scope, &shape, id);
                }
            }
        }
        for p in sig.parents.clone() {
            let SigType::Ref { sym, .. } = &p else {
                continue;
            };
            // Library parents were the original reason this exists (see the
            // doc comment). A class that came in on `-cp` needs its *own*
            // library's parents just as much, because the class file cannot
            // write the type arguments the pickle does. `java_parents` reads
            // them from the `Signature` attribute, which is post-typer: the
            // superclass recorded there for a class extending a
            // `@specialized` one is the specialized subclass
            // (`DriverJdbcType$mcJ$sp extends DriverJdbcType<Object>`), so
            // slick's `LongJdbcType` arrived as a `JdbcType[Any]` and no
            // search for `BaseTypedType[Long]` could ever succeed. The
            // pickle says `DriverJdbcType[Long]`, which is what nsc reads.
            // Prelude classes stay on the old rule: their hierarchy is
            // hand-written and the rest of the typer reasons about it.
            if !sym.starts_with("scala.") && class_sym.0 < st.prelude_end {
                continue;
            }
            let Some(t) = self.conv(st, bin, &scope, &p) else {
                trace(format_args!("{full}: parent {sym} unconvertible"));
                continue;
            };
            // `extends AnyVal` lives *only* in the pickle. A value class's
            // class file has `java/lang/Object` for a superclass and no
            // interfaces, so `java_parents` reads `AnyRef` and nothing else --
            // and `SymbolTable::is_value_class`, which asks for an `AnyVal`
            // parent, answered no for every value class that arrives on `-cp`.
            // The whole of erasure hangs off that answer: `Stream$.fromIterator`
            // really has the descriptor `()Z`, and its result was being cast to
            // `Stream$PartiallyAppliedFromIterator` and called as an instance.
            // Prelude models (StringOps, ArrayOps, DurationInt) retain their
            // chosen representation. Other value classes, including ones in
            // scala-library, use the same recovered representation as -cp APIs.
            if matches!(t, Type::AnyVal) {
                // Duration syntax is installed lazily, after prelude_end, and
                // NewWrapper already boxes it. Ordinary erasure would box it
                // twice. This exception belongs to those three explicit models,
                // not to every value class packaged in scala-library.
                if crate::prelude_durrange::uses_boxed_conversion(&st.get(class_sym).jvm_name) {
                    continue;
                }
                if class_sym.0 >= st.prelude_end
                    && !st.get(class_sym).parents.iter().any(|q| {
                        matches!(q, Type::AnyVal)
                            || st.class_sym_of(q).is_some_and(|c| c == st.anyval_sym)
                    })
                {
                    trace(format_args!("{full}: attaching pickled parent AnyVal"));
                    st.get_mut(class_sym).parents.push(Type::AnyVal);
                }
                if class_sym.0 >= st.prelude_end {
                    let underlying = sig
                        .members
                        .iter()
                        .find(|m| m.kind == MemberKind::Def && m.name == "<init>")
                        .and_then(|m| read_shape(&m.ty))
                        .and_then(|shape| {
                            shape
                                .clauses
                                .first()
                                .and_then(|clause| clause.params.first())
                                .map(|param| param.ty.clone())
                        })
                        .and_then(|ty| self.conv(st, bin, &scope, &ty));
                    ensure_value_class_field(st, bin, class_sym, underlying);
                }
                continue;
            }
            // Function parents are represented structurally by `conv`, but
            // inheritance requires the actual FunctionN class and arguments.
            // Retaining the Scala parent list across JVM completion otherwise
            // drops Function1[Int, Symbol] from VarArityClassApi entirely.
            let t = st.function_class_form(&t).unwrap_or(t);
            let Type::Class { sym: psym, .. } = &t else {
                continue;
            };
            if *psym == class_sym {
                continue;
            }
            // The same class, already above this one but with *different*
            // arguments, is the erased generic signature of a class file
            // talking: `ArrayBuilder<T> implements ReusableBuilder<T, Object>`
            // is what javac's format can say, while the pickle says
            // `ReusableBuilder[T, Array[T]]` -- and `To` is invariant, so
            // `ArrayBuilder[E]` was not a `Builder[E, Array[E]]` and
            // `mutable.ArrayBuilder.make[E]` could not be returned as one.
            // The pickle is scalac's own record of the declaration, so it
            // refines the parent it agrees with on the class. Never a prelude
            // class: its hierarchy is hand-written and reasoned about.
            //
            // A `@specialized` variant counts as the same parent. The class
            // file of slick's `LongJdbcType` names
            // `DriverJdbcType$mcJ$sp` for a superclass, a class the pickle
            // does not contain at all (specialization runs after pickling),
            // and leaving it in place alongside the pickled
            // `DriverJdbcType[Long]` would put two instantiations of one
            // class in the same hierarchy -- with `JdbcType[Any]` still
            // reachable, so `x: BaseTypedType[Any] = longColumnType` would go
            // on being accepted. It is replaced, not added to.
            let pjvm = st.get(*psym).jvm_name.clone();
            let existing = st.get(class_sym).parents.iter().position(|q| {
                matches!(q, Type::Class { sym: q, .. }
                    if *q == *psym
                        || despecialized(&st.get(*q).jvm_name).is_some_and(|b| b == pjvm))
            });
            if let Some(i) = existing {
                let refines = class_sym.0 >= st.prelude_end
                    && !matches!(&st.get(class_sym).parents[i], Type::Class { args, .. }
                        if *args == parent_args(&t));
                if refines {
                    trace(format_args!(
                        "{full}: refining parent {} to {}",
                        st.get(*psym).name,
                        st.display_type(&t)
                    ));
                    st.get_mut(class_sym).parents[i] = t;
                }
                continue;
            }
            // A parent that already has this class above it would make the
            // hierarchy cyclic; the prelude's own shape wins.
            if inherits_from(st, *psym, class_sym) {
                continue;
            }
            trace(format_args!(
                "{full}: attaching pickled parent {}",
                st.display_type(&t)
            ));
            st.get_mut(class_sym).parents.push(t);
        }
    }

    /// The symbol for a library class named in a pickle, creating a stub from
    /// the class's own signature if the symbol table does not have it.
    ///
    /// Without this, any member whose signature mentions a class the prelude
    /// never declared (`scala.collection.IndexedSeq`, `scala.math.Numeric`)
    /// is declined, which is most of the collection API. The stub carries the
    /// real JVM name and the right number of type parameters, which is what
    /// the typer needs to name the type and the backend needs to emit it.
    ///
    /// It deliberately does **not** carry the class's parents: giving a stub a
    /// parent chain would change subtyping for everything, and the prelude's
    /// own hierarchy is the one existing programs are checked against. The cost
    /// is that a stubbed type is only usable as itself (see README).
    pub(crate) fn ensure_class(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        full_name: &str,
        module: bool,
    ) -> Option<SymbolId> {
        let internal = full_name.replace('.', "/");
        // A pickle writes `Outer.Inner` and `pkg.Class` the same way, so the
        // JVM name of a nested class (`Outer$Inner`) cannot be read off the
        // dotted name. Take the first candidate that is really a classfile;
        // `key` decides both the symbol's `jvm_name` and its owner, and
        // getting it wrong invents a package called `Names`.
        let key = {
            let plain = if module {
                format!("{internal}$")
            } else {
                internal.clone()
            };
            scala_rs_pickle::sym::pickle_files_for(full_name, module)
                .into_iter()
                // `pickle_files_for` also offers the *enclosing top-level*
                // class file, because that is where a nested class's pickle
                // actually lives. That candidate is right for reading bytes and
                // wrong for naming a class: accepting it made
                // `scala.reflect.api.Constants.Constant` -- an abstract type
                // member, with no class file of its own -- resolve to the
                // enclosing trait `Constants`, so `Literal(Constant(42))` was
                // checked against a parameter of the wrong type and no erased
                // descriptor matched. A candidate only names this class if it
                // ends with this class's own simple name.
                .filter(|c| names_class(c, full_name))
                .find(|c| bin.find_class(c).ok().flatten().is_some())
                .unwrap_or(plain)
        };
        if let Some(id) = self.stubs.get(&key).copied() {
            self.rehome_static_nested_module(st, bin, id, &key, module);
            return Some(id);
        }
        // A Scala pickle may name a Java generic directly. Complete that
        // classfile here: returning an existing or new placeholder is not
        // enough because conversion of this very signature needs the class's
        // type-parameter arity. TwirlHelperImports is the concrete case --
        // its implicit `java.lang.Iterable[T] =>
        // scala.collection.Iterable[T]` was dropped as unmappable when
        // `Iterable` had not happened to be loaded first. Java classfiles are
        // authoritative for this namespace, and their installer preserves
        // the exact JVM identity and generic parents.
        if full_name.starts_with("java.") || full_name.starts_with("javax.") {
            let bytes = bin.find_class(&key).ok().flatten()?;
            let class = crate::javaclass::parse_java_classfile(&bytes).ok()?;
            let id = crate::classpath::install_java_class(st, &class);
            self.stubs.insert(key, id);
            return Some(id);
        }
        // A symbol already in the table wins, whatever shape it is in. An
        // earlier version gave an under-specified one (`scala/collection/Seq`,
        // entered by `find_or_stub_java_class` with no type parameters) the
        // parameters its pickle declares, so that `Seq[B]` would match it. That
        // unlocked `diff` / `intersect` / `union` / `indexOfSlice`, but it also
        // mutates a symbol the prelude built and the rest of the typer already
        // reasons about: with `Seq` reshaped, `xs.segmentLength(_ < 3)` and
        // `xs.scanRight(0)(_ + _)` -- both hand-written prelude members --
        // stopped resolving. Breaking a member that works is worse than not
        // supplying one that does not, so the table is left alone.
        if let Some(id) = crate::classpath::find_by_jvm(st, &key) {
            self.rehome_static_nested_module(st, bin, id, &key, module);
            self.stubs.insert(key.clone(), id);
            self.give_stub_its_kinds(st, bin, id, full_name, module);
            if !full_name.starts_with("scala.") {
                // A signature referenced this class while its own pickle was
                // being read. JVM metadata can complete its other parents,
                // but must not replace the Scala parent arguments already
                // recovered from that pickle with erased Object arguments.
                let scala_parents = self
                    .parented
                    .contains(&id.0)
                    .then(|| st.get(id).parents.clone());
                crate::classpath::install_classpath_metadata(st, bin, id);
                if let Some(parents) = scala_parents {
                    st.get_mut(id).parents = parents;
                }
            }
            // A classfile descriptor can introduce ReusableBuilder as a bare
            // placeholder before its Scala signature is needed. `Vector`
            // returns `ReusableBuilder[A, Vector[A]]`, whose pickled parent is
            // `Builder[A, Vector[A]]` even though the JVM placeholder carried
            // no generic hierarchy. Complete only this placeholder: applying
            // arbitrary pickle parents to hand-written collection models
            // changes their self-type result inference.
            if key == "scala/collection/mutable/ReusableBuilder" {
                self.attach_parents(st, bin, id, full_name, module);
            }
            return Some(id);
        }
        // Outside the library, hand the placeholder to the ordinary classfile
        // loader rather than building one here. `find_or_stub_java_class`
        // marks it `JAVA`, which is what makes `ensure_java_loaded` complete it
        // from its own classfile the moment the program names it; a stub built
        // below would keep its empty member list forever. It still gets its
        // kinds now, since that is what the signature being converted is about
        // to apply.
        if !full_name.starts_with("scala.") {
            // A Scala nested class may have no ScalaSignature of its own: the
            // enclosing class carries the pickle for source aliases, while
            // `Owner$Empty.class` still carries the constructor and members
            // needed by a nullary alias such as `type EmptyAlias = Empty`.
            // Materialize that ordinary classfile here instead of declining
            // the alias RHS and leaving the imported type as an unresolved
            // prefix refinement. Top-level Scala classes must continue
            // through the signature-aware placeholder path below; eagerly
            // installing their classfile would discard ScalaSignature data.
            if is_nested_jvm_name(&key) && !self.has_pickle(bin, full_name, module) {
                if let Ok(Some(bytes)) = bin.find_class(&key) {
                    if let Ok(class) = crate::javaclass::parse_java_classfile(&bytes) {
                        let id = crate::classpath::install_java_class(st, &class);
                        if st.get(id).jvm_name == key {
                            self.stubs.insert(key, id);
                            self.give_stub_its_kinds(st, bin, id, full_name, module);
                            return Some(id);
                        }
                    }
                }
            }
            if !self.has_pickle(bin, full_name, module) {
                return None;
            }
            let id = crate::classpath::find_or_stub_scala_class(st, &key);
            // That function also answers by simple name, and a companion's
            // placeholder -- `Async` standing for `.../Async$` -- is not the
            // trait this signature names. Handing it back made
            // `type Async[F[_]] = cats.effect.kernel.Async[F]` resolve to a
            // class with no members at all, so `F.unit` stopped being one.
            if st.get(id).jvm_name != key {
                return None;
            }
            self.rehome_static_nested_module(st, bin, id, &key, module);
            self.stubs.insert(key, id);
            self.give_stub_its_kinds(st, bin, id, full_name, module);
            crate::classpath::install_classpath_metadata(st, bin, id);
            return Some(id);
        }
        let sig = {
            let mut src = BinSource(bin);
            self.sigs.class_sig(&mut src, full_name, module).ok()?
        };
        let base = key.strip_suffix('$').unwrap_or(&key).to_string();
        // A nested class belongs to the class that encloses it, not to the
        // package. Splitting `scala/collection/Iterator$GroupedIterator` at
        // the last `/` alone entered a *package-level* class whose simple name
        // was `Iterator$GroupedIterator`, which is not a name any Scala source
        // can write and, worse, is a **second symbol** for a class the
        // class-file loader enters correctly as `GroupedIterator` inside
        // `Iterator`. Which of the two a program got depended on which path
        // reached the class first: name `Iterator.GroupedIterator` in a
        // signature and `install_java_class_in` builds the member; let
        // `it.sliding(n)`'s pickled result type arrive first and this built the
        // package-level twin, whose parent list is `AnyRef` — so
        // `it.sliding(n).map(f)` reported `found: (Seq[A]) => B  required:
        // (A) => Any` and `Iterator[A].sliding(n)` conformed to no `Iterator`
        // at all. [`crate::classpath::java_class_owner`] is the rule the rest
        // of the compiler already uses to decide who owns a nested JVM class,
        // and both paths now agree on one symbol.
        let simple = crate::classpath::java_simple_name(&base);
        if simple.is_empty() {
            return None;
        }
        // The rule is one rule for both halves of this repair, and it is
        // decided here, from names alone, before any symbol exists: a *nested
        // collection*. `pickle_reaches` walks the pickled parent names to
        // `scala.collection.IterableOnce`.
        //
        // Everything about the restriction is measured. Lifting it to every
        // nested library class costs `engine.rs::rd_reify_shape_expands_and_runs`
        // -- `scala.reflect`'s API is built by hand in `prelude_reflect` and
        // reasoned about by `reify*.rs` and `macros.rs`, so a second
        // `Exprs.Expr` under the owner the JVM name implies makes
        // `c.universe.Expr.apply[Int](...)` bind the one with no members -- and
        // attaching those classes' pickled parents costs nine more workspace
        // tests and two slick errors in `ShapedValue.scala`'s quasiquote.
        // Lifting it to every nested `scala.collection` class costs
        // `fvg.rs::map_with_filter_overloads_match_scalac`: `MapOps.WithFilter`
        // is nested there and is not an `IterableOnce`, and
        // `val flatPairs: Map[String, Int] = m.withFilter(p).flatMap { case
        // (k, v) => List(k -> v) }` -- which scalac accepts -- becomes
        // `found: Iterable[(String, Int)]`.
        //
        // The element and the `CC` this repair is about are read off a
        // collection's base type, so a nested collection is exactly the family
        // that needs the hierarchy, and it is the family whose symbol identity
        // a program can observe. `docs/not-implemented.md` records what is left.
        let nested = is_nested_jvm_name(&base)
            && base.starts_with("scala/collection/")
            && self.pickle_reaches(bin, full_name, module, "scala.collection.IterableOnce");
        // A class nested in an object-only module has an unambiguous source
        // owner even outside collections.  In particular TailCalls$TailRec
        // is `TailCalls.TailRec`; flattening it into the package as a class
        // literally named `TailCalls$TailRec` writes a different Scala type
        // to our pickle.  Companion pairs remain on the conservative path:
        // `java_class_owner` chooses their ordinary class half, so this only
        // admits an owner already known to be a module class.
        let object_nested = if is_nested_jvm_name(&base) {
            let owner = crate::classpath::java_class_owner(st, &base);
            (st.get(owner).kind == SymKind::ModuleClass).then_some(owner)
        } else {
            None
        };
        let (owner, simple) = if nested || object_nested.is_some() {
            let o = object_nested.unwrap_or_else(|| crate::classpath::java_class_owner(st, &base));
            if o.is_none() || !st.get(o).is_class_like() {
                return None;
            }
            (o, simple)
        } else {
            let (pkg_jvm, flat) = match base.rsplit_once('/') {
                Some((p, n)) => (p.to_string(), n.to_string()),
                None => (String::new(), base.clone()),
            };
            (crate::classpath::ensure_package(st, &pkg_jvm), flat)
        };
        let id = if module {
            let cls = st.alloc(
                format!("{simple}$"),
                owner,
                SymKind::ModuleClass,
                Flags::MODULE.with(Flags::FINAL),
                &key,
            );
            let m = st.alloc(&simple, owner, SymKind::Module, Flags::MODULE, &key);
            st.get_mut(m).ty = Type::ModuleRef(cls);
            st.get_mut(cls).ty = Type::ModuleRef(cls);
            cls
        } else {
            let mut flags = Flags::EMPTY;
            if sig.flags & pflags::TRAIT != 0 || sig.flags & pflags::INTERFACE != 0 {
                flags = flags.with(Flags::INTERFACE).with(Flags::TRAIT);
            }
            if sig.flags & pflags::ABSTRACT != 0 {
                flags = flags.with(Flags::ABSTRACT);
            }
            let id = st.alloc(&simple, owner, SymKind::Class, flags, &key);
            let tparams: Vec<SymbolId> = sig
                .tparams
                .iter()
                .map(|tp| {
                    let t = st.alloc(&tp.name, id, SymKind::TypeParam, variance_flags(tp), "");
                    st.get_mut(t).ty = Type::TypeParam(t);
                    set_tparam_kind(st, t, tp);
                    t
                })
                .collect();
            st.get_mut(id).tparams = tparams;
            st.get_mut(id).parents = vec![Type::AnyRef];
            st.get_mut(id).ty = Type::Class {
                sym: id,
                args: vec![],
            };
            id
        };
        trace(format_args!("stubbed class {full_name} (module={module})"));
        // Before anything that can convert another signature: `attach_parents`
        // below reaches `conv_ref` for each parent, and a hierarchy that names
        // this class again must find the symbol rather than build a second one.
        self.stubs.insert(key.clone(), id);
        self.stub_superclass_from_classfile(st, bin, id, &key);
        // `ReusableBuilder` is introduced from `Vector.newBuilder`'s return
        // type before any member lookup asks for it. Its JVM classfile names
        // only the erased `Builder` parent, while the pickle carries the
        // applied parent that makes `ReusableBuilder[A, To]` conform to
        // `Builder[A, To]`. Complete this single collection placeholder here;
        // the broader nested-collection path below remains intentionally
        // limited because arbitrary pickle parents can change source-visible
        // overloads for classes the prelude models by hand.
        if key == "scala/collection/mutable/ReusableBuilder" {
            self.attach_parents(st, bin, id, full_name, module);
        }
        // `stub_superclass_from_classfile` declines a nested class that has
        // type parameters, because a class file cannot say what arguments its
        // superclass is applied at. Its pickle can, and this is the one place
        // that has already opened it. Without this the stub stays at `AnyRef`:
        // `Iterator[A].sliding(n)` returns `Iterator.GroupedIterator[B]`, which
        // extends `AbstractIterator[Seq[B]]`, and with no parents it conformed
        // to no `Iterator` at all -- so `.map(f)` read the element off the
        // receiver's own first argument and asked for `(A) => Any` where the
        // element is `Seq[A]`.
        if nested {
            self.attach_parents(st, bin, id, full_name, module);
            // And transitively, because one hop is not a hierarchy.
            // `GroupedIterator` gains `AbstractIterator[Seq[B]]`, whose own
            // stub is still standing at `AnyRef`, so `Iterator` is still not a
            // base class and `it.sliding(n)` conforms to no `Iterator`.
            // `ensure_parents` memoises in `self.parented` and is what a member
            // lookup on any of these classes would have run anyway, so the walk
            // is this hierarchy and not the library.
            let mut queue: Vec<SymbolId> = parent_classes(st, id);
            let mut steps = 0;
            while let Some(p) = queue.pop() {
                if self.parented.contains(&p.0) {
                    continue;
                }
                self.ensure_parents(st, bin, p);
                queue.extend(parent_classes(st, p));
                steps += 1;
                if steps > 256 {
                    break;
                }
            }
        }
        Some(id)
    }

    /// Put a static object nested in a companion object under that companion's
    /// module class, even when an earlier classpath scan attached its binary
    /// class to the ordinary class half.
    ///
    /// Both static and per-instance nested objects use an `Outer$Inner$`
    /// class name.  The bytecode ABI distinguishes them: only the static
    /// object owns a static `MODULE$` field.  Keeping the static object under
    /// `Outer` makes its members look instance-bound and prevents macro tree
    /// transport from spelling the stable path `Outer.Inner.member`.
    fn rehome_static_nested_module(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        module_class: SymbolId,
        internal: &str,
        requested_module: bool,
    ) {
        if !requested_module
            || !is_nested_jvm_name(internal)
            || st.get(module_class).kind != SymKind::ModuleClass
        {
            return;
        }
        let is_static = self.java_class(bin, internal).is_some_and(|jc| {
            jc.fields.iter().any(|f| {
                f.name == "MODULE$" && f.access & 0x0008 != 0 && f.desc == format!("L{internal};")
            })
        });
        if !is_static {
            return;
        }
        st.get_mut(module_class).flags = st.get(module_class).flags.with(Flags::STATIC);
        let old_owner = st.get(module_class).owner;
        if old_owner.is_none() || st.get(old_owner).kind != SymKind::Class {
            return;
        }
        let new_owner = st.companion_module_class_for_implicits(old_owner);
        if new_owner == old_owner || new_owner.is_none() {
            return;
        }
        let module = st.get(old_owner).members.iter().copied().find(|id| {
            st.get(*id).kind == SymKind::Module && st.module_class_of(*id) == module_class
        });
        st.get_mut(old_owner)
            .members
            .retain(|id| *id != module_class && Some(*id) != module);
        st.get_mut(module_class).owner = new_owner;
        if !st.get(new_owner).members.contains(&module_class) {
            st.get_mut(new_owner).members.push(module_class);
        }
        if let Some(module) = module {
            st.get_mut(module).owner = new_owner;
            if !st.get(new_owner).members.contains(&module) {
                st.get_mut(new_owner).members.push(module);
            }
        }
    }

    /// Whether `full_name`'s pickled parent chain reaches `target`, read from
    /// names alone.
    ///
    /// Called while deciding how to enter a class that has no symbol yet, so it
    /// cannot ask the symbol table -- and it must not build anything, because
    /// what it answers is *how* to build. `class_sig` is memoised in
    /// `self.sigs`, and the chains it walks are a handful of nodes.
    fn pickle_reaches(
        &mut self,
        bin: &mut BinaryIndex,
        full_name: &str,
        module: bool,
        target: &str,
    ) -> bool {
        let mut seen: Vec<String> = Vec::with_capacity(16);
        let mut work: Vec<(String, bool)> = vec![(full_name.to_string(), module)];
        let mut steps = 0;
        while let Some((name, is_module)) = work.pop() {
            steps += 1;
            if steps > 256 {
                return false;
            }
            if seen.contains(&name) {
                continue;
            }
            seen.push(name.clone());
            let Ok(sig) = ({
                let mut src = BinSource(bin);
                self.sigs.class_sig(&mut src, &name, is_module)
            }) else {
                continue;
            };
            for p in &sig.parents {
                let SigType::Ref { sym, .. } = p else {
                    continue;
                };
                if sym == target {
                    return true;
                }
                work.push((sym.clone(), false));
            }
        }
        false
    }

    /// Give a freshly stubbed class the superclass its *class file* names, when
    /// that class is already in the table.
    ///
    /// A stub starts at `AnyRef` and stays there unless some *member* is later
    /// looked up on it, since `ensure_parents` is only reached from member
    /// completion. A class that is nothing but a *type* therefore conformed to
    /// nothing: `Duration.MinusInf` has the pickled type `Duration.Infinite`,
    /// and cats-kernel's `override def minBound: Duration = Duration.MinusInf`
    /// was `type mismatch; found: Duration$Infinite  required: Duration`.
    ///
    /// Deliberately narrow, in three ways, because this runs in the middle of
    /// converting some *other* class's signature:
    ///
    ///   * only a **nested** class (`Outer$Inner`). A top-level one is named by
    ///     source and gets its parents from `ensure_parents` when a member is
    ///     first looked up on it; giving `scala.concurrent.duration
    ///     .FiniteDuration`'s stub its parent here instead made
    ///     `FiniteDuration(2L, SECONDS)` stop conforming to a `FiniteDuration`
    ///     parameter. `Duration.Infinite` is never selected on, so nothing ever
    ///     reaches it that way.
    ///   * only the **class file**, never a second pickle read from inside the
    ///     walk in progress.
    ///   * only the **superclass**, and only one already in the table: nothing
    ///     here creates a symbol, and an interface (or a parent with type
    ///     parameters) cannot be named without arguments the class file does
    ///     not carry.
    fn stub_superclass_from_classfile(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        cls: SymbolId,
        internal: &str,
    ) {
        if !is_nested_jvm_name(internal) {
            return;
        }
        // With type parameters of its own the class file cannot say what
        // arguments the parent is applied at; only the pickle can.
        if !st.get(cls).tparams.is_empty() {
            return;
        }
        let Some(sup) = self
            .java_class(bin, internal)
            .and_then(|c| c.super_name.clone())
        else {
            return;
        };
        if sup == "java/lang/Object" {
            return;
        }
        let Some(parent) = crate::classpath::find_by_jvm(st, &sup) else {
            return;
        };
        if parent == cls || !st.get(parent).tparams.is_empty() {
            return;
        }
        trace(format_args!(
            "{internal}: superclass {sup} from its class file"
        ));
        st.get_mut(cls).parents = vec![Type::Class {
            sym: parent,
            args: Vec::new(),
        }];
    }

    /// Give a *placeholder* symbol the type parameters its pickle declares.
    ///
    /// `find_or_stub_java_class` enters a bare symbol for every name a parent
    /// list or a descriptor mentions, without reading the classfile. Outside
    /// the library those placeholders are what a pickled signature keeps
    /// running into: `cats.effect.kernel.Sync` is entered from `Ref`'s parent
    /// list with no type parameters at all, so `Sync[F]` is "applied to 1
    /// argument but the symbol has 0" and every `Ref.of` / `Ref.ofEffect` /
    /// `Ref.lens` is declined for it.
    ///
    /// Only a symbol nothing has filled in yet, and never one the *prelude*
    /// built: a class the typer has really loaded already has its parameters,
    /// and reshaping a prelude class is what `ensure_class` deliberately
    /// refuses to do.
    ///
    /// "Prelude" is the line, not the `scala.` package. `find_or_stub_java_class`
    /// enters library names too -- `scala/collection/mutable/ReusableBuilder`
    /// comes in from `ArrayBuilder`'s classfile parent list, with no type
    /// parameters and no members -- and refusing those left
    /// `ReusableBuilder[T, Array[T]]` "applied to 2 arguments but the symbol
    /// has 0", so `ArrayBuilder` never got the parent that makes it a
    /// `Builder[E, Array[E]]`. A symbol allocated after `prelude_end` was
    /// built by no hand the rest of the typer reasons about.
    fn give_stub_its_kinds(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        id: SymbolId,
        full_name: &str,
        module: bool,
    ) {
        let jvm = st.get(id).jvm_name.clone();
        if jvm.starts_with("java/") || jvm.starts_with("javax/") {
            return;
        }
        if jvm.starts_with("scala/") && id.0 < st.prelude_end {
            return;
        }
        let Ok(sig) = ({
            let mut src = BinSource(bin);
            self.sigs.class_sig(&mut src, full_name, module)
        }) else {
            return;
        };
        // A placeholder built by the class-file loader (`find_or_stub_java_class`)
        // does not know a Scala trait from a class, and the backend reads
        // exactly that to choose `invokeinterface` over `invokevirtual`:
        // `scala.reflect.macros.Universe` reached through a descriptor came
        // out unmarked, and every `u.Literal(...)` a macro implementation
        // compiled to an `invokevirtual` the JVM rejects with
        // `IncompatibleClassChangeError`. Only ever *adds* the flag, and only
        // when the pickle says so.
        if sig.flags & pflags::TRAIT != 0 || sig.flags & pflags::INTERFACE != 0 {
            let f = st.get(id).flags;
            st.get_mut(id).flags = f.with(Flags::INTERFACE).with(Flags::TRAIT);
        }
        // A signature can mention a value class before any expression uses
        // its members. Recover its underlying field now: descriptor matching
        // must already erase a direct parameter to that field's JVM type.
        if sig
            .parents
            .iter()
            .any(|parent| matches!(parent, SigType::Ref { sym, .. } if sym == "scala.AnyVal"))
        {
            self.attach_parents(st, bin, id, full_name, module);
        }
        // Members are not the test for "already filled in": a class the JVM
        // loader completed from its class file has methods and still no type
        // parameters, and `cats.FlatMap` in that state made every
        // `FlatMap[F]` in cats' syntax layer an arity error -- which of the
        // two happened first depended only on the order of the file's imports.
        // JVM signatures supply parameter identities but cannot encode
        // higher kinds or Scala variance. Complete those facts in place even
        // when metadata discovery has already installed the parameters.
        if sig.tparams.is_empty() {
            return;
        }
        trace(format_args!(
            "{full_name}: giving the placeholder symbol its {} pickled type parameter(s)",
            sig.tparams.len()
        ));
        adopt_tparam_kinds(st, id, &sig);
    }

    /// The declared descriptor of `name`, searched from `internal` up through
    /// superclasses and interfaces. `want` is one slot per value parameter,
    /// holding the erased descriptor we expect where we can name it.
    ///
    /// Returns `None` when nothing matches, or when candidates still tie after
    /// the parameter descriptors are compared: picking one arbitrarily would
    /// silently call the wrong method.
    fn erased_desc(
        &mut self,
        bin: &mut BinaryIndex,
        internal: &str,
        name: &str,
        want: &[Option<String>],
    ) -> Option<ErasedDecl> {
        self.erased_desc_return(bin, internal, name, want, None)
    }

    fn erased_desc_return(
        &mut self,
        bin: &mut BinaryIndex,
        internal: &str,
        name: &str,
        want: &[Option<String>],
        result: Option<&str>,
    ) -> Option<ErasedDecl> {
        let arity = want.len();
        let mut seen: HashSet<String> = HashSet::new();
        // Each frontier entry remembers whether it was reached through a hop
        // the JVM cannot make -- see `ErasedDecl::off_the_bytecode_path`.
        let mut level = vec![(internal.to_string(), false)];
        for _ in 0..32 {
            if level.is_empty() {
                return None;
            }
            let mut hits: Vec<ErasedDecl> = Vec::new();
            let mut next = Vec::new();
            for (cn, off_path) in &level {
                let off_path = *off_path;
                if !seen.insert(cn.clone()) {
                    continue;
                }
                let Some(jc) = self.java_class(bin, cn) else {
                    continue;
                };
                let declared_by_interface = crate::javaclass::is_java_interface(jc.access);
                let mut parents: Vec<String> = Vec::new();
                for jm in &jc.methods {
                    if jm.name != name || jm.access & (ACC_BRIDGE | ACC_SYNTHETIC | ACC_STATIC) != 0
                    {
                        continue;
                    }
                    if desc_arity(&jm.desc) == Some(arity)
                        && params_match(&jm.desc, want)
                        && !hits.iter().any(|h| h.desc == jm.desc)
                    {
                        hits.push(ErasedDecl {
                            desc: jm.desc.clone(),
                            declared_in: cn.clone(),
                            declared_by_interface,
                            off_the_bytecode_path: off_path,
                        });
                    }
                }
                if let Some(s) = &jc.super_name {
                    if s != "java/lang/Object" {
                        parents.push(s.clone());
                    }
                }
                parents.extend(jc.interfaces.iter().cloned());
                let bare = parents.is_empty();
                if jc.super_name.is_some() && bare {
                    parents.push("java/lang/Object".to_string());
                }
                // A Scala trait whose members are all abstract can compile to
                // an interface that lists no parents at all --
                // `api/JavaUniverse.class` declares four methods and
                // `interfaces: 0`, though the trait extends `Universe`. Its
                // ancestry then exists only in the pickle. Only that case is
                // topped up: reading the pickled parents of a class whose
                // bytecode *does* declare its hierarchy widens the search to
                // ancestors the JVM does not see, and the extra `map`
                // descriptors that turns up make the answer ambiguous
                // (`Map#map` regresses).
                next.extend(parents.into_iter().map(|p| (p, off_path)));
                if bare {
                    next.extend(
                        self.pickled_parent_files(bin, cn)
                            .into_iter()
                            .map(|p| (p, true)),
                    );
                }
            }
            match hits.len() {
                0 => {}
                1 => return Some(hits.remove(0)),
                _ => {
                    // JVM overloads can differ only in their erased result.
                    // IntMap.map has both its own IntMap-returning method and
                    // a MapOps forwarder with identical Function1 arguments.
                    let expected = result?;
                    hits.retain(|hit| {
                        hit.desc
                            .rsplit_once(')')
                            .is_some_and(|(_, ret)| ret == expected)
                    });
                    return (hits.len() == 1).then(|| hits.remove(0));
                }
            }
            level = next;
        }
        None
    }

    /// The classfiles of the parents `internal`'s *pickle* declares.
    ///
    /// Empty for anything outside the standard library and for any class whose
    /// pickle is missing: this only tops up the bytecode hierarchy, it never
    /// replaces it.
    fn pickled_parent_files(&mut self, bin: &mut BinaryIndex, internal: &str) -> Vec<String> {
        if !internal.starts_with("scala/") {
            return Vec::new();
        }
        let module = internal.ends_with('$');
        let full = internal.trim_end_matches('$').replace('/', ".");
        let sig = {
            let mut src = BinSource(bin);
            match self.sigs.class_sig(&mut src, &full, module) {
                Ok(s) => s,
                Err(_) => return Vec::new(),
            }
        };
        let mut out = Vec::new();
        for p in &sig.parents {
            let SigType::Ref { sym, .. } = p else {
                continue;
            };
            if let Some(f) = self.class_file_of(bin, sym) {
                out.push(f);
            }
        }
        out
    }

    /// Whether a pickle describes `full_name`.
    /// Whether this module may read `class_sym`'s pickle at all.
    ///
    /// Scoped to the standard library, plus any class `adopt_binary_class` has
    /// taken over: those two are the pickles the typer reads. A plain Java
    /// classfile on `-cp` has no pickle and keeps its own path in through
    /// `install_java_class`.
    ///
    /// Note that asking for a member of a class this answers `false` for is
    /// not merely useless but *harmful*: [`Self::complete_named`] memoizes the
    /// refusal, and the memo then stands in for the pickled signature once the
    /// class really is adopted. See the blocking-slick entry in
    /// `docs/gitbucket.md`.
    pub(crate) fn pickle_readable(&self, st: &SymbolTable, class_sym: SymbolId) -> bool {
        st.get(class_sym).jvm_name.starts_with("scala/")
            || self.adopted.contains(&class_sym.0)
            || self.implicits_supplied.contains(&class_sym.0)
    }

    /// Whether a nested Scala class's enclosing top-level class carries the
    /// pickle that describes it.  Scala 2 emits an empty `Scala` attribute on
    /// nested classfiles and stores all nested class signatures in the
    /// top-level `ScalaSignature`; the class-only `pickle_readable` predicate
    /// cannot see that because it deliberately has no BinaryIndex argument.
    fn nested_pickle_readable(
        &mut self,
        st: &SymbolTable,
        bin: &mut BinaryIndex,
        class_sym: SymbolId,
    ) -> bool {
        if class_sym.is_none() || !st.get(class_sym).is_class_like() {
            return false;
        }
        let internal = st.get(class_sym).jvm_name.as_str();
        if internal.is_empty()
            || internal.starts_with("java/")
            || internal.starts_with("javax/")
            || !is_nested_jvm_name(internal)
        {
            return false;
        }
        let is_module = st.get(class_sym).kind == SymKind::ModuleClass;
        let Some(full) = self.pickled_full_name(bin, internal, is_module) else {
            return false;
        };
        // `pickled_full_name` can resolve the nested dotted name through its
        // enclosing class even when the nested classfile itself has no
        // signature.  Re-read the signature here only as a readability test;
        // the subsequent complete_named lookup uses the same cached value.
        self.has_pickle(bin, &full, is_module)
    }

    /// The pickled signature of a library (or adopted) class or module class,
    /// found under its dotted name the way `complete_named` finds it (the
    /// `$` spelling first, then the nested one). `None` for a class this run
    /// does not read pickles for.
    pub(crate) fn class_sig_of(
        &mut self,
        st: &SymbolTable,
        bin: &mut BinaryIndex,
        cls: SymbolId,
    ) -> Option<std::rc::Rc<scala_rs_pickle::sym::ClassSig>> {
        if cls.is_none() || !self.pickle_readable(st, cls) {
            return None;
        }
        let sym = st.get(cls);
        if !sym.is_class_like() {
            return None;
        }
        let is_module = sym.kind == SymKind::ModuleClass;
        let plain = sym.jvm_name.trim_end_matches('$').replace('/', ".");
        let mut src = BinSource(bin);
        if let Ok(sig) = self.sigs.class_sig(&mut src, &plain, is_module) {
            return Some(sig);
        }
        let dotted = scala_rs_pickle::names::nested_to_dotted(&plain);
        if dotted != plain {
            if let Ok(sig) = self.sigs.class_sig(&mut src, &dotted, is_module) {
                return Some(sig);
            }
        }
        None
    }

    /// Whether a pickled result type declares `name`; `None` means that the
    /// result has no readable pickle and must therefore be retained
    /// conservatively by extension-candidate warming.
    fn pickled_result_has_member(
        &mut self,
        bin: &mut BinaryIndex,
        result: &str,
        name: &str,
    ) -> Option<bool> {
        let full = self.pickled_full_name(bin, result, false)?;
        let (hits, _errs) = {
            let mut src = BinSource(bin);
            self.sigs.lookup(&mut src, &full, false, name)
        };
        Some(!hits.is_empty())
    }

    /// The pickled signature of the library class `full_name` (dotted).
    pub(crate) fn class_sig_by_name(
        &mut self,
        bin: &mut BinaryIndex,
        full_name: &str,
        module: bool,
    ) -> Option<std::rc::Rc<scala_rs_pickle::sym::ClassSig>> {
        let mut src = BinSource(bin);
        self.sigs.class_sig(&mut src, full_name, module).ok()
    }

    fn has_pickle(&mut self, bin: &mut BinaryIndex, full_name: &str, module: bool) -> bool {
        let mut src = BinSource(bin);
        let r = self.sigs.class_sig(&mut src, full_name, module);
        if let Err(e) = &r {
            trace(format_args!(
                "has_pickle {full_name} module={module}: {e:?}"
            ));
        }
        r.is_ok()
    }

    /// A Scala object's static-forwarder classfile does not declare a class
    /// in its ScalaSignature. The JVM view must not be imported as a source
    /// type when the object itself is already available.
    pub(crate) fn is_module_only_mirror(
        &mut self,
        st: &SymbolTable,
        bin: &mut BinaryIndex,
        id: SymbolId,
    ) -> bool {
        if st.get(id).kind != SymKind::Class
            || st.get(id).flags.contains(Flags::JAVA)
            || st.is_source_class(id)
        {
            return false;
        }
        let internal = &st.get(id).jvm_name;
        if internal.is_empty() || internal.ends_with('$') {
            return false;
        }
        let full = internal.replace('/', ".");
        !self.has_pickle(bin, &full, false) && self.has_pickle(bin, &full, true)
    }

    /// The classfile that really holds `full_name`, or `None` if there is none.
    fn class_file_of(&mut self, bin: &mut BinaryIndex, full_name: &str) -> Option<String> {
        scala_rs_pickle::sym::pickle_files_for(full_name, false)
            .into_iter()
            .find(|c| bin.find_class(c).ok().flatten().is_some())
    }

    fn java_class(&mut self, bin: &mut BinaryIndex, internal: &str) -> Option<&JavaClass> {
        if !self.classes.contains_key(internal) {
            let parsed = bin
                .find_class(internal)
                .ok()
                .flatten()
                .and_then(|b| crate::javaclass::parse_classfile_for_scala_signature(&b).ok());
            self.classes.insert(internal.to_string(), parsed);
        }
        self.classes.get(internal).and_then(|c| c.as_ref())
    }
}

// ---------------------------------------------------------------------------
// Pickled signature -> method shape
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct ShapeTParam {
    name: String,
    lo: Option<SigType>,
    hi: Option<SigType>,
    /// The binders inside a higher-kinded parameter's `PolyType`-shaped
    /// bounds.  Their names are the vocabulary of `lo` / `hi`: ScalaTest's
    /// `OPT[e] <: Option[e]`, for example, cannot be converted with only the
    /// enclosing method's `E` and `OPT` in scope.
    inner: Vec<ShapeTParam>,
    variance: i8,
}

#[derive(Debug)]
struct Param {
    name: String,
    ty: SigType,
    by_name: bool,
    /// Raw pickled flags, for `DEFAULTPARAM`.
    flags: u64,
}

#[derive(Debug)]
struct Clause {
    params: Vec<Param>,
    implicit: bool,
}

#[derive(Debug)]
struct Shape {
    tparams: Vec<ShapeTParam>,
    clauses: Vec<Clause>,
    ret: SigType,
    /// The member is an `implicit def` / `implicit val`. Only the pickle says
    /// so — a classfile has no bit for it — and without it a jar's companion
    /// object contributes nothing to implicit search (`Async[IO]` is
    /// `IO.asyncForIO`, and that is the only place it lives).
    implicit: bool,
}

impl Shape {
    fn arity(&self) -> usize {
        self.clauses.iter().map(|c| c.params.len()).sum()
    }
}

/// Read nsc's pickled macro `signature` into the two facts the expander needs.
///
/// The signature is one fingerprint per *implementation* parameter, clause by
/// clause. The first clause is the implementation's `(c: Context)` and is not
/// an argument of the macro application; the trailing `WeakTypeTag`s are not
/// either. What is left lines up one for one with the call site's arguments,
/// and `LIFTED_TYPED` marks the ones the implementation takes as `c.Expr[T]`
/// rather than as a bare `c.Tree` -- the same distinction
/// `Typer::macro_impl_expr_args` reads off a source-level implementation's
/// parameter types.
///
/// Returns `(tag_params, expr_args)`, or `None` for a signature this reading
/// does not cover: no `Context` clause at all, or a tag in the middle of the
/// value parameters, where "drop the trailing tags" would be a guess.
/// A readable name for a pickled type that did not convert, for a diagnostic.
fn sig_spelling(t: &SigType) -> String {
    match t {
        SigType::Ref { sym, args } if args.is_empty() => sym.clone(),
        SigType::Ref { sym, args } => format!("{sym}[{}]", args.len()),
        _ => "a type scala-rs cannot read out of the pickle".to_string(),
    }
}

fn macro_signature_shape(
    signature: &[Vec<i32>],
    is_bundle: bool,
) -> Option<(Vec<usize>, Vec<bool>)> {
    use scala_rs_pickle::sym::fingerprint;
    let rest = if is_bundle {
        signature
    } else {
        let (context, rest) = signature.split_first()?;
        // A vanilla implementation takes Context in its first clause.
        if context.len() != 1 || context[0] != fingerprint::UNDETERMINED {
            return None;
        }
        rest
    };
    let flat: Vec<i32> = rest.iter().flatten().copied().collect();
    let tag_params = flat.iter().rev().take_while(|&&f| f >= 0).count();
    let values = &flat[..flat.len() - tag_params];
    if values.iter().any(|&f| f >= 0) {
        return None;
    }
    Some((
        // The fingerprint of a tag parameter is not a flag: it is the *index*
        // of the implementation type parameter the tag is for, which is also
        // the index into the type arguments written on the implementation
        // reference (`MacroImpl::targs`). It used to be counted and dropped.
        flat[flat.len() - tag_params..]
            .iter()
            .map(|&f| f as usize)
            .collect(),
        values
            .iter()
            .map(|&f| f == fingerprint::LIFTED_TYPED)
            .collect(),
    ))
}

/// Peel `POLYtpe` / `METHODtpe` layers into type parameters and parameter
/// clauses. nsc writes a parameterless `def` as a `POLYtpe` with no type
/// parameters (`NullaryMethodType`), which becomes an empty clause list.
fn read_shape(t: &SigType) -> Option<Shape> {
    let mut tparams = Vec::new();
    let mut clauses = Vec::new();
    let mut cur = t;
    let mut guard = 0;
    loop {
        guard += 1;
        if guard > 16 {
            return None;
        }
        match cur {
            SigType::Poly {
                tparams: tps,
                result,
            } => {
                for tp in tps {
                    tparams.push(shape_tparam(tp));
                }
                cur = result;
            }
            SigType::Method {
                params,
                implicit,
                result,
            } => {
                clauses.push(Clause {
                    params: params
                        .iter()
                        .map(|p| Param {
                            name: p.name.clone(),
                            ty: p.ty.clone(),
                            by_name: p.by_name,
                            flags: p.flags,
                        })
                        .collect(),
                    implicit: *implicit,
                });
                cur = result;
            }
            other => {
                return Some(Shape {
                    tparams,
                    clauses,
                    ret: other.clone(),
                    implicit: false,
                })
            }
        }
    }
}

fn shape_tparam(tp: &scala_rs_pickle::sym::TParam) -> ShapeTParam {
    let (inner, bounds) = match &tp.bounds {
        SigType::Poly { tparams, result } => {
            (tparams.iter().map(shape_tparam).collect(), result.as_ref())
        }
        bounds => (Vec::new(), bounds),
    };
    let (lo, hi) = match bounds {
        SigType::Bounds { lo, hi } => (Some((**lo).clone()), Some((**hi).clone())),
        _ => (None, None),
    };
    ShapeTParam {
        name: tp.name.clone(),
        lo,
        hi,
        inner,
        variance: tp.variance,
    }
}

fn alloc_shape_tparam(st: &mut SymbolTable, owner: SymbolId, shape: &ShapeTParam) -> SymbolId {
    let flags = match shape.variance {
        1 => Flags::COVARIANT,
        -1 => Flags::CONTRAVARIANT,
        _ => Flags::EMPTY,
    };
    let id = st.alloc(&shape.name, owner, SymKind::TypeParam, flags, "");
    st.get_mut(id).ty = Type::TypeParam(id);
    let inner = shape
        .inner
        .iter()
        .map(|p| alloc_shape_tparam(st, id, p))
        .collect();
    st.get_mut(id).tparams = inner;
    id
}

/// Whether two shapes take the same value parameters, clause by clause.
fn same_value_params(a: &Shape, b: &Shape) -> bool {
    a.clauses.len() == b.clauses.len()
        && a.clauses.iter().zip(&b.clauses).all(|(a, b)| {
            a.implicit == b.implicit
                && a.params.len() == b.params.len()
                && a.params
                    .iter()
                    .zip(&b.params)
                    .all(|(a, b)| a.ty == b.ty && a.by_name == b.by_name)
        })
}

fn sig_type_is_owner_tparam_application(t: &SigType, owner_tparams: &HashSet<String>) -> bool {
    match t {
        SigType::Annotated(inner) => sig_type_is_owner_tparam_application(inner, owner_tparams),
        SigType::Ref { sym, args } => owner_tparams.contains(sym) && !args.is_empty(),
        _ => false,
    }
}

/// Decide what to do with a pickled type parameter that no *explicit*
/// parameter mentions, and refuse the member when nothing can determine it.
///
/// `def max[B >: A](implicit ord: Ordering[B]): A` and
/// `def sorted[B >: A](implicit ord: Ordering[B]): C` are the shape this is
/// named for. This function used to substitute `B := A` and drop the type
/// parameter, on the theory that scalac resolves `B` to its lower bound.
/// **It does not.** `B` stays undetermined until the implicit clause is
/// solved, so an explicitly supplied argument determines it:
/// `xs.sorted(AA.toOrdering)` with `AA >: A` is `B := AA` and compiles, and
/// scalac's `-Xprint:typer` prints the instantiation. Pinning `B` to `A`
/// first made us reject that with `found: Ordering[AA] required: Ordering[A]`
/// — six of cats' errors, every one of them a `.sorted` call. Keeping the
/// parameter is what the typer wants: `infer_method_tparams_in` already
/// solves `B` from an explicit argument, and falls back to the lower bound
/// when the argument comes from implicit search (`xs.max` on a `List[Int]`
/// still finds `Ordering.Int`), which is the behaviour a source-declared
/// `def maxX[B >: A](implicit ord: Ordering[B]): A` has always had here.
///
/// What remains is the refusal. A type parameter that *nothing* can determine
/// — no explicit parameter, no lower bound, not named by the result, not what
/// an implicit is asked for — makes the whole member ineligible, because the
/// typer would silently eta-expand `xs.max` into a function value rather than
/// fail. That shape never reaches the user.
fn pin_undetermined_tparams(shape: Shape) -> Option<Shape> {
    let determined: HashSet<String> = shape
        .clauses
        .iter()
        .filter(|c| !c.implicit)
        .flat_map(|c| c.params.iter())
        .flat_map(|p| mentioned(&p.ty))
        .collect();
    let mut pin: HashMap<String, SigType> = HashMap::new();
    let mut kept = Vec::new();
    for tp in &shape.tparams {
        let keep = |kept: &mut Vec<ShapeTParam>| {
            kept.push(ShapeTParam {
                name: tp.name.clone(),
                lo: tp.lo.clone(),
                hi: tp.hi.clone(),
                inner: tp.inner.clone(),
                variance: tp.variance,
            })
        };
        if determined.contains(&tp.name) {
            keep(&mut kept);
            continue;
        }
        let named_by_an_implicit = shape
            .clauses
            .iter()
            .any(|c| c.params.iter().any(|p| mentioned(&p.ty).contains(&tp.name)));
        let real_lo = matches!(&tp.lo, Some(lo) if !matches!(lo, SigType::Ref { sym, .. } if sym == "scala.Nothing"));
        // An implicit parameter names it and it has a lower bound: keep it.
        // An argument supplied explicitly for that clause is what determines
        // it -- `xs.sorted(AA.toOrdering)` with `AA >: A` is `B := AA` -- and
        // when the argument comes from implicit search instead, the typer
        // instantiates the parameter at its lower bound first
        // (`pin_lower_bounded_implicit_tparams`), which is what makes
        // `xs.max` on a `List[Int]` find `Ordering.Int`.
        if real_lo && named_by_an_implicit {
            keep(&mut kept);
            continue;
        }
        // A result parameter can still be fixed by explicit type arguments,
        // an expected result, or an apply on the returned value. Its lower
        // bound constrains inference; it must not replace the declaration.
        if mentioned(&shape.ret).contains(&tp.name) {
            keep(&mut kept);
            continue;
        }
        if real_lo {
            pin.insert(tp.name.clone(), tp.lo.clone().expect("real_lo"));
            continue;
        }
        // A *materialiser*: the whole member is one implicit clause, and the
        // type parameter is what the implicit is asked for.
        // `def typeOf[T](implicit ttag: TypeTag[T]): Type`, `symbolOf[T]`,
        // `weakTypeOf[T]` -- the three slick's macro implementations are
        // written with. These are always called with an explicit type argument
        // (`symbolOf[R]`), which is exactly what the branch above accepts for
        // `classTag[Short]`; the only difference is that the result type does
        // not happen to name `T`. With no explicit type argument `T` is
        // `Nothing` and the implicit search fails, which is a diagnostic, not a
        // wrong program.
        if shape.clauses.iter().all(|c| c.implicit) && named_by_an_implicit {
            keep(&mut kept);
            continue;
        }
        // Named by an implicit clause *behind an explicit one*: nsc leaves it
        // undetermined through the application and the implicit search solves
        // it (`Context.undetparams`), which `implicit_fit_open` /
        // `search_implicit_undet` do here too. slick's
        // `AnyOptionExtensionMethods.getOrElse[M, P2 <: P](default: M)(implicit
        // shape: Shape[FlatShapeLevel, M, _, P2], ol: OptionLift[P2, O]): P`
        // is the shape: `P2` is whatever the `Shape` found for `M` packs to.
        // A clause that is never filled is `reject_unapplied_implicit_clause`'s
        // missing implicit, not an eta-expansion. The explicit clause may be
        // empty: `def make[T: ClassTag]()` still has the application `()`
        // before its synthesized implicit clause, and that application is the
        // source-level boundary the classfile fallback cannot preserve.
        if named_by_an_implicit && shape.clauses.iter().any(|c| !c.implicit) {
            keep(&mut kept);
            continue;
        }
        // A type parameter the signature never mentions again: no parameter and
        // no result names it, so nothing the call site does depends on how it is
        // solved and there is no implicit to fail. nsc's *default getters* are
        // where this shape comes from -- they inherit the method's type
        // parameters whether or not the default expression uses them, so
        // `def halt[T: Manifest](status: Integer = null, body: T = (),
        // headers: Map[...] = ...)` gives `halt$default$1[T]: Integer`.
        // Declining that getter declined `halt` itself (`install` refuses a
        // member whose default it cannot fill), which is how `halt(400)` was
        // left with only the unrelated `halt(ActionResult)` overload.
        if !named_by_an_implicit {
            keep(&mut kept);
            continue;
        }
        // Unconstrained and undeterminable: refuse the member rather than hand
        // the typer something it will silently eta-expand.
        return None;
    }
    if pin.is_empty() {
        return Some(shape);
    }
    Some(Shape {
        tparams: kept,
        implicit: shape.implicit,
        clauses: shape
            .clauses
            .iter()
            .map(|c| Clause {
                implicit: c.implicit,
                params: c
                    .params
                    .iter()
                    .map(|p| Param {
                        name: p.name.clone(),
                        ty: scala_rs_pickle::sym::apply_subst(&p.ty, &pin),
                        by_name: p.by_name,
                        flags: p.flags,
                    })
                    .collect(),
            })
            .collect(),
        ret: scala_rs_pickle::sym::apply_subst(&shape.ret, &pin),
    })
}

/// The variance flags for `tp`, as `SLS 4.5` records them and `is_sub_type`
/// reads them (`Flags::COVARIANT` / `Flags::CONTRAVARIANT`).
///
/// A JVM generic signature cannot write variance at all -- it is a
/// compile-time-only concept, erased before the `Signature` attribute is
/// produced -- so a class installed purely from a class file (or stubbed
/// through `find_or_stub_java_class`) has every type parameter invariant.
/// `Outcome[+A]`'s pickle is the only place `A`'s variance survives; without
/// this, `case object Canceled extends Outcome[Nothing]`'s parent
/// `Outcome[Nothing]` failed `is_sub_type` against `Outcome[Int]` (invariant
/// comparison demands `Nothing =:= Int`), surfacing as "type mismatch; found:
/// Canceled$ required: Outcome[Int]" for every case object nested in a
/// companion whose trait a jar declares covariant.
fn variance_flags(tp: &scala_rs_pickle::sym::TParam) -> Flags {
    if tp.variance > 0 {
        Flags::COVARIANT
    } else if tp.variance < 0 {
        Flags::CONTRAVARIANT
    } else {
        Flags::EMPTY
    }
}

/// Give a binary class's type parameters the kinds its pickle declares.
///
/// A JVM generic signature writes `trait Monad[F[_]]` as `<F:Ljava/lang/Object;>`
/// — `F` is a proper type there, and there is no way to say otherwise. So the
/// symbols are already allocated, in the right order and under the right names;
/// what they are missing is the parameters *they* take. If the two disagree
/// about how many there are, the JVM signature is left alone: a mismatch means
/// this is not the class the pickle describes.
fn adopt_tparam_kinds(
    st: &mut SymbolTable,
    class_sym: SymbolId,
    sig: &scala_rs_pickle::sym::ClassSig,
) {
    let ids = st.get(class_sym).tparams.clone();
    if ids.is_empty() {
        // No generic signature at all (or none we parsed): the pickle is the
        // only description there is.
        if sig.tparams.is_empty() {
            return;
        }
        let mut fresh = Vec::new();
        for tp in &sig.tparams {
            let id = st.alloc(
                &tp.name,
                class_sym,
                SymKind::TypeParam,
                variance_flags(tp),
                "",
            );
            st.get_mut(id).ty = Type::TypeParam(id);
            set_tparam_kind(st, id, tp);
            fresh.push(id);
        }
        st.get_mut(class_sym).tparams = fresh;
        return;
    }
    if ids.len() != sig.tparams.len() {
        return;
    }
    for (id, tp) in ids.into_iter().zip(&sig.tparams) {
        set_tparam_kind(st, id, tp);
        st.get_mut(id).flags = st.get(id).flags.with(variance_flags(tp));
        adopt_primitive_bound(st, id, tp);
    }
}

/// A class type parameter bounded by a primitive value class (`class P[A <:
/// Int]`) erases to that primitive: the constructor is `P(int)` and the
/// accessor `a()I`. The JVM generic signature cannot say so -- nsc writes
/// `<A:Ljava/lang/Object;>` -- so a class read from `-cp` came without the
/// bound, and the erasure of its members' `A` fell back to `Object`: the call
/// site boxed the argument of `new P(3)` for a descriptor that takes an `int`
/// (`VerifyError`, against scalac's classes and our own alike). Only the
/// primitive bounds are taken from the pickle here; every other bound is
/// either written in the generic signature or erases to a reference the
/// descriptor already carries.
fn adopt_primitive_bound(st: &mut SymbolTable, id: SymbolId, tp: &scala_rs_pickle::sym::TParam) {
    if st.get(id).bound_hi.is_some() {
        return;
    }
    let SigType::Bounds { hi, .. } = &tp.bounds else {
        return;
    };
    let SigType::Ref { sym, args } = hi.as_ref() else {
        return;
    };
    if !args.is_empty() {
        return;
    }
    let prim = match sym.as_str() {
        "scala.Int" => Type::Int,
        "scala.Long" => Type::Long,
        "scala.Double" => Type::Double,
        "scala.Float" => Type::Float,
        "scala.Boolean" => Type::Boolean,
        "scala.Byte" => Type::Byte,
        "scala.Short" => Type::Short,
        "scala.Char" => Type::Char,
        _ => return,
    };
    st.get_mut(id).bound_hi = Some(prim);
}

/// Copy a type parameter's kind from the pickle. Names are immaterial to
/// `kind_arity`, but the tree is recursive: `F[_[_]]` needs the placeholder
/// for `F`'s first parameter to itself be a constructor. The JVM signature
/// only records the outer parameter count, so a binary class starts with this
/// information missing at every depth.
fn set_tparam_kind(st: &mut SymbolTable, id: SymbolId, tp: &scala_rs_pickle::sym::TParam) {
    let SigType::Poly { tparams, .. } = &tp.bounds else {
        return;
    };
    if tparams.is_empty() {
        return;
    }
    let existing = st.get(id).tparams.clone();
    let inner = if existing.is_empty() {
        let inner: Vec<SymbolId> = tparams
            .iter()
            .map(|tp| {
                let x = st.alloc(&tp.name, id, SymKind::TypeParam, variance_flags(tp), "");
                st.get_mut(x).ty = Type::TypeParam(x);
                x
            })
            .collect();
        st.get_mut(id).tparams = inner.clone();
        inner
    } else if existing.len() == tparams.len() {
        existing
    } else {
        // The erased classfile and pickle disagree about this kind. Preserve
        // the existing view rather than silently replacing unrelated symbols.
        return;
    };
    for (inner_id, inner_tp) in inner.into_iter().zip(tparams) {
        set_tparam_kind(st, inner_id, inner_tp);
    }
}

/// A type parameter's own kind arity. nsc pickles `F[_]` as a `POLYtpe` over
/// the bounds, so the number of quantified names is the arity; a proper type
/// parameter has plain `Bounds` and arity 0.
fn tparam_arity(tp: &scala_rs_pickle::sym::TParam) -> usize {
    match &tp.bounds {
        SigType::Poly { tparams, .. } => tparams.len(),
        _ => 0,
    }
}

/// Peel a pickled method type to the class named by its result.  Implicit
/// conversions have a `Poly`/`Method` spine, while their result is normally a
/// fully-qualified `Ref` such as `Foo.StringOps`.
fn sig_result_class_name(t: &SigType) -> Option<&str> {
    match t {
        SigType::Poly { result, .. }
        | SigType::Method { result, .. }
        | SigType::Existential { result, .. }
        | SigType::Annotated(result) => sig_result_class_name(result),
        SigType::Ref { sym, .. } if sym.contains('.') => Some(sym),
        _ => None,
    }
}

fn sig_parent_class_name(t: &SigType) -> Option<String> {
    match t {
        SigType::Annotated(inner) => sig_parent_class_name(inner),
        SigType::Ref { sym, .. } if sym.contains('.') => Some(sym.clone()),
        _ => None,
    }
}

/// Every bare name a type mentions, so we can tell which type parameters an
/// explicit argument would determine.
fn mentioned(t: &SigType) -> Vec<String> {
    let mut out = Vec::new();
    walk(t, &mut out, 0);
    out
}

fn walk(t: &SigType, out: &mut Vec<String>, depth: u32) {
    if depth > 24 {
        return;
    }
    let d = depth + 1;
    match t {
        SigType::Ref { sym, args } => {
            out.push(sym.clone());
            for a in args {
                walk(a, out, d);
            }
        }
        SigType::Annotated(x) => walk(x, out, d),
        SigType::Bounds { lo, hi } => {
            walk(lo, out, d);
            walk(hi, out, d);
        }
        SigType::Method { params, result, .. } => {
            for p in params {
                walk(&p.ty, out, d);
            }
            walk(result, out, d);
        }
        SigType::Poly { result, .. } | SigType::Existential { result, .. } => walk(result, out, d),
        SigType::Refined { parents, decls } => {
            for p in parents {
                walk(p, out, d);
            }
            for m in decls {
                walk(&m.ty, out, d);
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// SigType -> scala_rs_parser::Type
// ---------------------------------------------------------------------------

/// Map a pickled type onto the typer's. `None` means "cannot express this",
/// and the caller then declines to supply the member.
impl PickleSupply {
    fn conv(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        scope: &HashMap<String, Type>,
        t: &SigType,
    ) -> Option<Type> {
        self.conv_at(st, bin, scope, t, 0)
    }

    fn conv_at(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        scope: &HashMap<String, Type>,
        t: &SigType,
        depth: u32,
    ) -> Option<Type> {
        if depth > 24 {
            return None;
        }
        let d = depth + 1;
        match t {
            SigType::StringConstant(value) => Some(Type::Constant(
                scala_rs_parser::ast::Lit::String(value.clone()),
            )),
            SigType::Constant(value) => {
                use scala_rs_parser::ast::Lit;
                use scala_rs_pickle::read::Constant;
                let value = match value {
                    Constant::Unit => Lit::Unit,
                    Constant::Boolean(v) => Lit::Boolean(*v),
                    Constant::Int(v) => Lit::Int(*v),
                    Constant::Long(v) => Lit::Long(*v),
                    Constant::Float(v) => Lit::Float(*v),
                    Constant::Double(v) => Lit::Double(*v),
                    Constant::Char(v) => Lit::Char(char::from_u32(*v)?),
                    Constant::Null => Lit::Null,
                    // Byte/Short and unresolved entry references cannot be
                    // represented faithfully by the parser's literal types.
                    _ => return None,
                };
                Some(Type::Constant(value))
            }
            SigType::Annotated(inner) => self.conv_at(st, bin, scope, inner, d),
            SigType::Poly { tparams, result } => {
                if tparams.is_empty() {
                    return self.conv_at(st, bin, scope, result, d);
                }
                // A PolyType nested in a type argument is a type lambda,
                // not a method signature. Keep its binders and expose captured
                // parameters as leading arguments so later substitution can
                // reach them (for example EitherT[F, E, *]).
                if matches!(
                    result.as_ref(),
                    SigType::Method { .. } | SigType::Bounds { .. }
                ) {
                    return None;
                }
                let id = st.alloc(
                    "<lambda>",
                    SymbolId::NONE,
                    SymKind::TypeMember,
                    Flags::EMPTY,
                    "",
                );
                let shapes: Vec<_> = tparams.iter().map(shape_tparam).collect();
                let own: Vec<_> = shapes
                    .iter()
                    .map(|s| alloc_shape_tparam(st, id, s))
                    .collect();
                let mut inner = scope.clone();
                for (tp, &sym) in tparams.iter().zip(&own) {
                    inner.insert(tp.name.clone(), Type::TypeParam(sym));
                }
                for (shape, &sym) in shapes.iter().zip(&own) {
                    self.resolve_shape_tparam_bounds(st, bin, &inner, shape, sym);
                }
                let body = self.conv_at(st, bin, &inner, result, d)?;
                let mut free = Vec::new();
                crate::check::collect_tparams(&body, &mut free);
                for &sym in &own {
                    for bound in [&st.get(sym).bound_lo, &st.get(sym).bound_hi]
                        .into_iter()
                        .flatten()
                    {
                        crate::check::collect_tparams(bound, &mut free);
                    }
                }
                free.retain(|p| !own.contains(p));
                st.get_mut(id).tparams = free.iter().copied().chain(own).collect();
                st.get_mut(id).ty = body;
                st.get_mut(id).is_type_alias = true;
                Some(crate::symbol::apply_type_ctor(
                    Type::TypeMember(id),
                    free.into_iter().map(Type::TypeParam).collect(),
                ))
            }
            SigType::Existential { quantified, result } => {
                let mut inner = scope.clone();
                let ids: Vec<_> = quantified
                    .iter()
                    .map(|q| {
                        let id = st.alloc(
                            &q.name,
                            SymbolId::NONE,
                            SymKind::TypeParam,
                            Flags::EMPTY,
                            "",
                        );
                        inner.insert(q.name.clone(), Type::TypeParam(id));
                        id
                    })
                    .collect();
                let params: Vec<_> = quantified
                    .iter()
                    .zip(ids)
                    .map(|(q, id)| {
                        let bounds = self.exist_wildcard(st, bin, &inner, q, d);
                        if let Type::BoundedWildcard { lo, hi } = &bounds {
                            st.get_mut(id).bound_lo = lo.as_deref().cloned();
                            st.get_mut(id).bound_hi = hi.as_deref().cloned();
                        }
                        (id, bounds)
                    })
                    .collect();
                let body = self.conv_at(st, bin, &inner, result, d)?;
                Some(SymbolTable::pack_existential(params, body))
            }
            SigType::Ref { sym, args } => self.conv_ref(st, bin, scope, sym, args, d, 0),
            // Keep the class named by the pickle. Usually it is the member's
            // declaring class, and `subst_as_seen_from` then turns it into the
            // actual receiver: `b ++= xs` on a `Builder[Int, List[Int]]`
            // yields that `Builder` even though `++=` is declared by
            // `Growable`.
            //
            // A nested API can instead return its *enclosing* instance:
            // `trait Profile { trait API { implicit val profile:
            // Profile.this.type } }`. Replacing every `This(Profile)` by the
            // class currently being installed made that result `API.this.type`.
            // The imported value was consequently visible but could not serve
            // as a `Profile`. Preserving `Profile` lets the receiver's outer
            // prefix rebind it to the concrete enclosing instance.
            //
            // `subst_as_seen_from` can only do that when it sees the declaring
            // class among the installing class's ancestors, and a jar class's
            // parents are attached one class at a time. `duplicate: this.type`
            // is declared by reflect's `TreeApi` and installed on `SelectApi`,
            // whose chain runs through `SymTreeApi` -- a class nothing else
            // completed, left with `AnyRef` as its only parent. The result then
            // stayed `TreeApi.this.type` instead of the receiver. Complete the
            // chain before handing out a `this.type` of an ancestor.
            SigType::This(owner) => {
                let this = self.ensure_class(st, bin, owner, false);
                if let (Some(this), Some(Type::Class { sym, .. })) = (this, self.self_ty.clone())
                {
                    if this != sym && !st.is_ancestor_of(this, sym) {
                        self.ensure_ancestor_parents(st, bin, sym);
                    }
                }
                this.map(Type::ThisType).or_else(|| match &self.self_ty {
                    Some(Type::Class { sym, .. }) => Some(Type::ThisType(*sym)),
                    other => other.clone(),
                })
            }
            SigType::Refined { parents, decls } => {
                self.conv_refined(st, bin, scope, parents, decls, d)
            }
            // `p.x.type`: a package object's `val Resource = cats.effect.
            // kernel.Resource` -- exactly the shape `cats.effect`'s real
            // package object uses to re-export the kernel module -- has this
            // as its inferred type, `sym` carrying the referent's full dotted
            // name ("cats.effect.kernel.Resource") the same way `Ref` does
            // for an ordinary class. `ensure_class(.., module: true)` finds
            // its module class exactly as `Ref` finds a class's; only that
            // shape (a *module's own* singleton type) is handled; a `.type`
            // that resolves to something other than a module (a local `val`,
            // `this.type`-like paths this pickle reader has not modelled) has
            // no counterpart to build and is declined like the rest.
            SigType::Single { prefix, sym } => {
                // `F.type` where `F` is a parameter of the member being
                // installed (`def apply[F[_]](implicit F: Async[F]): F.type`):
                // the parameter's own type is what that singleton widens to.
                if let Some((t, parameter)) = self.param_singleton(sym) {
                    return Some(if parameter.is_none() {
                        t
                    } else {
                        Type::SingleType {
                            prefix: Box::new(Type::NoType),
                            sym: parameter,
                        }
                    });
                }
                // nsc's pickle printer can encode a nested module's owner in
                // the `Single` symbol using the JVM spelling of the final
                // segment as well as the source spelling.  For example the
                // case object `slick.ast.ColumnOption.PrimaryKey` may arrive
                // as `slick.ast.ColumnOption.ColumnOption$PrimaryKey` even
                // though its class file is `ColumnOption$PrimaryKey$.class`.
                // The first spelling makes `pickle_files_for` look for a
                // package named `ColumnOption` and leaves the singleton
                // unmappable, widening `O.PrimaryKey` to `Any`.  Try the
                // source-style nested spelling after the original so this is
                // still conservative for ordinary `$`-bearing names.
                let singleton_names = singleton_type_names(sym);
                let mut module = None;
                for name in singleton_names {
                    if let Some(cls) = self.ensure_class(st, bin, &name, true) {
                        module = Some(cls);
                        break;
                    }
                }
                if let Some(cls) = module {
                    // A member module belongs to the particular enclosing
                    // instance named by its singleton prefix. Static package
                    // modules have no such path-dependent identity.
                    let owner = st.get(cls).owner;
                    trace(format_args!(
                        "singleton {sym}: prefix={prefix:?} owner={owner:?} flags={:?} module={:?}",
                        st.get(cls).flags,
                        st.companion_module(cls)
                    ));
                    if !owner.is_none()
                        && st.get(owner).is_class_like()
                        && !st.get(cls).flags.contains(Flags::STATIC)
                    {
                        let module = st.companion_module(cls).unwrap_or(cls);
                        if let Some(pre) = self.conv_at(st, bin, scope, prefix, d) {
                            return Some(Type::SingleType {
                                prefix: Box::new(pre),
                                sym: module,
                            });
                        }
                    }
                    return Some(Type::ModuleRef(cls));
                }
                // Not a module: a `val`'s singleton type, which is what
                // slick's `val O: self.columnOptions.type = columnOptions`
                // pickles as. There is no singleton type to build here, but
                // the val's *declared* type has exactly the members a
                // selection off it reaches, and that is what the reference is
                // for. Without it the whole member was declined and `O` kept
                // the class file's erased accessor -- `O.PrimaryKey` was
                // "value PrimaryKey is not a member of RelationalTableComponent".
                self.conv_val_widening(st, bin, sym, d)
            }
            // The remaining forms (`super`, bare bounds, literal types) have
            // no faithful counterpart here yet.
            _ => None,
        }
    }

    /// The wildcard one quantified variable of an existential stands for.
    ///
    /// `_ >: Nothing <: Any` is the plain `Wildcard`; anything narrower keeps
    /// the bound, because that is the only thing a unification against the
    /// wildcard's position can read. A bound this reader cannot convert (a
    /// higher-kinded quantified variable, an F-bound naming the variable
    /// itself) falls back to the plain wildcard rather than guessing.
    fn exist_wildcard(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        scope: &HashMap<String, Type>,
        q: &scala_rs_pickle::sym::TParam,
        d: u32,
    ) -> Type {
        let SigType::Bounds { lo, hi } = &q.bounds else {
            return Type::Wildcard;
        };
        let lo = self
            .conv_at(st, bin, scope, lo, d)
            .filter(|t| !matches!(t, Type::Nothing));
        let hi = self
            .conv_at(st, bin, scope, hi, d)
            .filter(|t| !matches!(t, Type::Any));
        if lo.is_none() && hi.is_none() {
            return Type::Wildcard;
        }
        Type::BoundedWildcard {
            lo: lo.map(Box::new),
            hi: hi.map(Box::new),
        }
    }

    /// The declared type of the `val` a pickled `p.x.type` points at.
    ///
    /// `sym` is the referent's full dotted path (`slick.relational.
    /// RelationalTableComponent.columnOptions`). Everything before the last
    /// segment names the class that declares it, which is looked up the same
    /// way any other member is; the answer is the val's *declared* type, not a
    /// singleton, because there is no singleton type to build here. That
    /// widening is what a selection off the reference can reach, which is all
    /// the reference is used for in a signature.
    ///
    /// Declined -- rather than guessed at -- when the owner has no pickle,
    /// when nothing of that name is declared, or when the entry takes
    /// parameters (a `def` is not a path).
    fn conv_val_widening(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        sym: &str,
        d: u32,
    ) -> Option<Type> {
        let (owner_name, member) = sym.rsplit_once('.')?;
        if member.is_empty() || owner_name.is_empty() {
            return None;
        }
        if !self.widening.insert(sym.to_string()) {
            return None;
        }
        let widened = self.conv_val_widening_inner(st, bin, owner_name, member, d);
        self.widening.remove(sym);
        widened
    }

    fn conv_val_widening_inner(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        owner_name: &str,
        member: &str,
        d: u32,
    ) -> Option<Type> {
        // A module class first only when the dotted name really is one: the
        // owner of `Outer.opts` is the trait `Outer`, not an object.
        let owner_is_module =
            !self.has_pickle(bin, owner_name, false) && self.has_pickle(bin, owner_name, true);
        if !owner_is_module && !self.has_pickle(bin, owner_name, false) {
            return None;
        }
        let jvm_member = scala_rs_pickle::names::encode_method_name(member);
        let (hits, _errs) = {
            let mut src = BinSource(bin);
            self.sigs
                .lookup(&mut src, owner_name, owner_is_module, &jvm_member)
        };
        let hit = hits.into_iter().find(|h| {
            matches!(h.member.kind, MemberKind::Val | MemberKind::Def) && h.member.is_public_api()
        })?;
        // A path element has no parameters. `Poly { tparams: [] }` is nsc's
        // spelling for a parameterless `def`, which is a path in nsc's sense
        // too (`def columnOptions: ColumnOptions`), so it is looked through.
        let ty = match &hit.member.ty {
            SigType::Poly { tparams, result } if tparams.is_empty() => (**result).clone(),
            SigType::Method { .. } | SigType::Poly { .. } => return None,
            other => other.clone(),
        };
        let owner = self.ensure_class(st, bin, owner_name, owner_is_module)?;
        // `this.type` inside that declaration means the class that declares
        // it, not whatever class the member being installed lives on.
        let saved = self.self_ty.replace(Type::Class {
            sym: owner,
            args: st
                .get(owner)
                .tparams
                .iter()
                .map(|t| Type::TypeParam(*t))
                .collect(),
        });
        let conv = self.conv_at(st, bin, &HashMap::new(), &ty, d);
        self.self_ty = saved;
        conv
    }

    /// `T { type A = U; def f: V }` — a `REFINEDtpe`.
    ///
    /// cats' whole syntax layer is written in these: simulacrum gives every
    /// `toFooOps` the result type `Foo.Ops[F, A] { type TypeClassType =
    /// Foo[F] }`, so declining the form meant declining `toFlatMapOps`,
    /// `toMonadOps`, `toApplicativeErrorOps` … and with them every extension
    /// method `import cats.syntax.all._` is supposed to bring into scope.
    ///
    /// Unlike [`Self::conv_upper_bound`], nothing here is dropped quietly: a
    /// parent or a declaration that does not convert declines the whole type,
    /// because these are the members a caller actually reaches through. The
    /// only simplification is the one that loses nothing -- a single parent
    /// with no declarations *is* that parent.
    fn conv_refined(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        scope: &HashMap<String, Type>,
        parents: &[SigType],
        decls: &[scala_rs_pickle::sym::Member],
        d: u32,
    ) -> Option<Type> {
        let mut ps: Vec<Type> = Vec::new();
        for p in parents {
            let Some(t) = self.conv_at(st, bin, scope, p, d) else {
                trace(format_args!("refinement: parent {p:?} does not convert"));
                return None;
            };
            // `Ops[F, A] with AnyRef` is `Ops[F, A]`; nsc writes the `AnyRef`
            // whenever the refinement has no other parent to stand on.
            if matches!(t, Type::AnyRef) && parents.len() > 1 || ps.contains(&t) {
                continue;
            }
            ps.push(t);
        }
        let mut ds: Vec<scala_rs_parser::RefineDecl> = Vec::new();
        for m in decls {
            let Some(decl) = self.conv_refine_decl(st, bin, scope, m, d) else {
                trace(format_args!(
                    "refinement: declaration {} ({:?}) does not convert: {:?}",
                    m.name, m.kind, m.ty
                ));
                return None;
            };
            ds.push(decl);
        }
        match (ps.len(), ds.is_empty()) {
            (0, _) => None,
            (1, true) => Some(ps.remove(0)),
            _ => Some(Type::Refined {
                parents: ps,
                decls: ds,
            }),
        }
    }

    /// One declaration inside a refinement. `None` for anything the typer's
    /// `RefineDecl` cannot hold (a polymorphic `def`, a nested class), so the
    /// caller declines the refinement rather than handing back a type that
    /// quietly lost a member.
    fn conv_refine_decl(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        scope: &HashMap<String, Type>,
        m: &scala_rs_pickle::sym::Member,
        d: u32,
    ) -> Option<scala_rs_parser::RefineDecl> {
        let name = m.name.clone();
        match m.kind {
            MemberKind::TypeAlias => Some(scala_rs_parser::RefineDecl::Type {
                name,
                rhs: Some(self.conv_at(st, bin, scope, &m.ty, d)?),
                tparams: 0,
                lo: None,
                hi: None,
            }),
            MemberKind::AbstractType => {
                let SigType::Bounds { lo, hi } = &m.ty else {
                    return None;
                };
                let lo = match lo.as_ref() {
                    SigType::Ref { sym, .. } if sym == "scala.Nothing" => None,
                    other => Some(self.conv_at(st, bin, scope, other, d)?),
                };
                let hi = match hi.as_ref() {
                    SigType::Ref { sym, .. } if sym == "scala.Any" => None,
                    other => Some(self.conv_upper_bound(st, bin, scope, other, d)?),
                };
                Some(scala_rs_parser::RefineDecl::Type {
                    name,
                    rhs: None,
                    tparams: 0,
                    lo,
                    hi,
                })
            }
            MemberKind::Val => Some(scala_rs_parser::RefineDecl::Val {
                name,
                ty: self.conv_at(st, bin, scope, &m.ty, d)?,
            }),
            MemberKind::Def => {
                // `Poly { tparams: [] }` is nsc's `NullaryMethodType`: a
                // parameterless `def`, not a polymorphic one. A `def` with
                // type parameters of its own has nowhere to put them.
                let mut ty = &m.ty;
                if let SigType::Poly { tparams, result } = ty {
                    if !tparams.is_empty() {
                        return None;
                    }
                    ty = result;
                }
                let mut paramss: Vec<Vec<Type>> = Vec::new();
                while let SigType::Method { params, result, .. } = ty {
                    let mut ps = Vec::with_capacity(params.len());
                    for p in params {
                        ps.push(self.conv_at(st, bin, scope, &p.ty, d)?);
                    }
                    paramss.push(ps);
                    ty = result;
                }
                Some(scala_rs_parser::RefineDecl::Def {
                    name,
                    // A pickled structural declaration's own type parameters
                    // are not read back yet; the pickle's `PolyType` wrapper is
                    // unwrapped above without them.
                    tparams: Vec::new(),
                    paramss,
                    ret: self.conv_at(st, bin, scope, ty, d)?,
                })
            }
            MemberKind::Class | MemberKind::Module => None,
        }
    }

    /// `want_arity` is the kind the *position* expects: 0 for an ordinary
    /// type, 1 for the argument of an `F[_]`-shaped parameter. Only a position
    /// that wants a constructor may be filled by an unapplied class.
    fn param_singleton(&self, path: &str) -> Option<(Type, SymbolId)> {
        // Reference paths use encoded operator names while method
        // declarations use source names, including in dependent results.
        let decoded = scala_rs_pickle::names::decode_method_name(path);
        let path = decoded.as_str();
        if let Some(ty) = self.param_singletons.get(path) {
            return self
                .param_singleton_symbols
                .get(path)
                .copied()
                .map(|sym| (ty.clone(), sym));
        }
        // A few older pickles use a JVM/source spelling for the same full
        // path. A suffix fallback remains safe only when it identifies one
        // formal parameter; never let HashMap iteration choose among several
        // candidates.
        let mut found = None;
        for (key, ty) in &self.param_singletons {
            if !path.ends_with(key) {
                continue;
            }
            let Some(sym) = self.param_singleton_symbols.get(key).copied() else {
                continue;
            };
            if found.is_some() {
                return None;
            }
            found = Some((ty.clone(), sym));
        }
        found
    }

    #[allow(clippy::too_many_arguments)]
    fn conv_ref(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        scope: &HashMap<String, Type>,
        sym: &str,
        args: &[SigType],
        d: u32,
        want_arity: usize,
    ) -> Option<Type> {
        // `p#T`, kept whole by the pickle reader when `p` is a type parameter
        // or an abstract type member. The prefix is what settles which `T`
        // this is; when it cannot be settled here, the member's own qualified
        // name is exactly the answer the reader used to give.
        if let Some((pre, member)) = sym.split_once('#') {
            // The parameter-path shortcut also retains an unapplied abstract
            // constructor such as evidence.F in a higher-kinded position. Applied
            // aliases need the general projection path to substitute their
            // binders: c.WeakTypeTag[R] must keep R, not the alias's own T.
            if args.is_empty() {
                if let Some(t) = self.param_projection(st, bin, pre, member) {
                    if st.kind_arity(&t) == want_arity {
                        trace(format_args!("projection {sym}: retained parameter path"));
                        return Some(t);
                    }
                }
            }
            if let Some(t) = self.conv_projection(st, bin, scope, pre, member, args, d) {
                trace(format_args!("projection {sym} -> {}", st.display_type(&t)));
                return Some(t);
            }
            let fallback = self.conv_ref(st, bin, scope, member, args, d, want_arity)?;
            // A lost alias binder is not the enclosing receiver's member.
            // Reading an unresolved T#Head as HList#Head silently makes every
            // heterogeneous index have the first element's type.
            if !pre.contains('.')
                && !scope.contains_key(pre)
                && matches!(&fallback, Type::TypeMember(_))
                && self
                    .self_ty
                    .as_ref()
                    .and_then(|t| st.class_sym_of(t))
                    .is_none_or(|owner| st.type_members_named(owner, pre).is_empty())
            {
                return None;
            }
            // The prefix is a type parameter, or a type member the class
            // leaves deferred: nothing here can settle it, but the class's
            // *users* can. `slick.lifted.TableQuery[E <: AbstractTable[_]]
            // extends Query[E, E#TableElementType, Seq]` is only a
            // `Query[Accounts, (String, Int), Seq]` because `Accounts` fixes
            // `TableElementType`, and answering with the bare declaration --
            // which is what dropping the prefix does -- is where the `Any`
            // in every downstream slick error came from. Keep the projection
            // and let `SymbolTable::subst_projections` reduce it at the
            // argument.
            if args.is_empty() {
                if let Type::TypeMember(decl) = fallback {
                    let prefix = Self::abstract_prefix_sym(st, scope, pre).or_else(|| {
                        // A class's abstract member need not have a lexical
                        // scope entry. Preserve Backend#Session as a projection
                        // too, so an overriding profile can refine Backend.
                        let root = self.completing_for.map(|sym| Type::Class {
                            sym,
                            args: Vec::new(),
                        });
                        let outer = self.self_ty.clone();
                        if let Some(root) = root {
                            self.self_ty = Some(root);
                        }
                        let found =
                            self.self_type_member_at(st, bin, scope, pre, &[], d + 1, false);
                        self.self_ty = outer;
                        match found {
                            Some(Type::TypeMember(id)) if st.is_deferred_type_member(id) => {
                                Some(id)
                            }
                            _ => None,
                        }
                    });
                    if let Some(p) = prefix {
                        let id = st.abstract_projection(p, decl);
                        trace(format_args!("projection {sym}: kept unreduced"));
                        return Some(Type::TypeMember(id));
                    }
                    if !scope.contains_key(pre) {
                        // The declaration alone cannot settle an unresolved
                        // prefix. E.g. T#Head inside a binary alias is not the
                        // current HCons receiver's Head. Decline this signature
                        // instead of supplying a falsely precise result type.
                        return None;
                    }
                }
            }
            // The same mistake the other way round: an abstract prefix (the
            // alias parameter `T` of `type HeadOf[T <: HList] = T#Head`)
            // whose unqualified member name was just read as the *receiver's*
            // concrete alias. An inherited alias is installed on the receiver
            // (`HCons`), where `Head` is `H`, so `HeadOf[X]` became `H` for
            // every `X` and `hlist(1)` got the first element's type. Keep the
            // projection on the prefix when its bound declares the member,
            // and decline otherwise.
            if args.is_empty() && !matches!(fallback, Type::TypeMember(_)) {
                if let Some(p) = Self::abstract_prefix_sym(st, scope, pre) {
                    let short = member.rsplit_once('.').map(|(_, m)| m).unwrap_or(member);
                    let decl = st
                        .get(p)
                        .bound_hi
                        .clone()
                        .and_then(|hi| st.class_sym_of(&hi))
                        .and_then(|owner| {
                            st.type_members_named(owner, short)
                                .into_iter()
                                .find(|&id| st.is_deferred_type_member(id))
                        });
                    let Some(decl) = decl else {
                        trace(format_args!(
                            "projection {sym}: abstract prefix, declined the receiver's reading"
                        ));
                        return None;
                    };
                    let id = st.abstract_projection(p, decl);
                    trace(format_args!("projection {sym}: kept unreduced on the prefix's bound"));
                    return Some(Type::TypeMember(id));
                }
            }
            trace(format_args!("projection {sym}: prefix does not settle it"));
            return Some(fallback);
        }
        if let Some(bound) = scope.get(sym) {
            if args.is_empty() {
                return Some(bound.clone());
            }
            // A type parameter applied to arguments (`F[A]` for an `F[_]`).
            // `Type::Applied` is exactly that, but only when the constructor
            // really takes that many parameters: a wildcard from an
            // existential, or a parameter whose kind we never recovered, would
            // make `F[A]` a type nothing can be checked against.
            let bound = bound.clone();
            if st.kind_arity(&bound) != args.len() {
                trace(format_args!(
                    "{sym}: applied to {} argument(s) but is not a constructor of that kind",
                    args.len()
                ));
                return None;
            }
            // Applying a higher-kinded type parameter retains the kinds of
            // its arguments too. In `Tuple[F]` where `Tuple[_[_]]`, the bare
            // `Rep` argument is a constructor, not an incomplete `Rep[?]`.
            // Converting all arguments as ordinary values loses that kind and
            // makes otherwise valid binary parents such as
            // `CaseClassShape[..., Tuple[Rep], ...]` unconvertible.
            let wants = st.tparam_arities(&bound);
            let mut a = Vec::with_capacity(args.len());
            for (i, arg) in args.iter().enumerate() {
                let want = wants.get(i).copied().unwrap_or(0);
                let converted = match arg {
                    SigType::Ref {
                        sym: arg_sym,
                        args: arg_args,
                    } => self.conv_ref(st, bin, scope, arg_sym, arg_args, d, want),
                    other => self.conv_at(st, bin, scope, other, d),
                }?;
                a.push(converted);
            }
            return Some(Type::Applied {
                ctor: Box::new(bound),
                args: a,
            });
        }
        match sym {
            "scala.Unit" => return Some(Type::Unit),
            "scala.Boolean" => return Some(Type::Boolean),
            "scala.Byte" => return Some(Type::Byte),
            "scala.Short" => return Some(Type::Short),
            "scala.Int" => return Some(Type::Int),
            "scala.Long" => return Some(Type::Long),
            "scala.Float" => return Some(Type::Float),
            "scala.Double" => return Some(Type::Double),
            "scala.Char" => return Some(Type::Char),
            "scala.Any" => return Some(Type::Any),
            "scala.Singleton" => {
                return Some(Type::Class {
                    sym: st.singleton_sym,
                    args: vec![],
                });
            }
            "scala.AnyRef" | "java.lang.Object" => return Some(Type::AnyRef),
            "scala.AnyVal" => return Some(Type::AnyVal),
            "scala.Nothing" => return Some(Type::Nothing),
            "scala.Null" => return Some(Type::Null),
            "java.lang.String" | "scala.Predef.String" => return Some(Type::String),
            "scala.Array" => {
                let a = self.conv_all(st, bin, scope, args, d)?;
                return a.into_iter().next().map(|e| Type::Array(Box::new(e)));
            }
            "scala.<byname>" => {
                let a = self.conv_all(st, bin, scope, args, d)?;
                return a.into_iter().next().map(|e| Type::ByName(Box::new(e)));
            }
            "scala.<repeated>" => {
                let a = self.conv_all(st, bin, scope, args, d)?;
                return a.into_iter().next().map(|e| Type::Repeated(Box::new(e)));
            }
            _ => {}
        }
        if let Some(n) = sym.strip_prefix("scala.Function") {
            if n.chars().all(|c| c.is_ascii_digit()) && !n.is_empty() {
                let mut a = self.conv_all(st, bin, scope, args, d)?;
                let ret = a.pop()?;
                return Some(Type::Function {
                    params: a,
                    ret: Box::new(ret),
                });
            }
        }
        if let Some(n) = sym.strip_prefix("scala.Tuple") {
            if n.chars().all(|c| c.is_ascii_digit()) && !n.is_empty() {
                return Some(Type::Tuple(self.conv_all(st, bin, scope, args, d)?));
            }
        }
        // An alias body is converted in the declaring class's vocabulary, so
        // its own type parameters are not in the method scope. Reconnect a
        // bare reference to that class parameter before trying classpath
        // completion or dependent value-parameter recovery (`type Packed =
        // Packed_` in a generic `Shape`).
        if args.is_empty() && scope.get(sym).is_none() {
            if let Some(Type::Class { sym: owner, .. }) = &self.self_ty {
                if let Some(tp) = st
                    .get(*owner)
                    .tparams
                    .iter()
                    .copied()
                    .find(|tp| st.get(*tp).name == sym)
                {
                    return Some(Type::TypeParam(tp));
                }
            }
        }
        // A type member named from inside its own class is pickled bare:
        // `Internals.internal` returns `Internal`, not `...Internals.Internal`.
        // There is no class by that name, so without this the member is
        // declined and `u.internal` -- the door to the whole reification API
        // -- is not a member of anything.
        //
        // A *parameterised* one is written the same way. slick's
        // `ImplicitColumnTypes` declares its twenty-four column types at
        // `BaseColumnType[T]`, a type member of the profile cake, so refusing
        // an applied one made every one of them an unmappable result type.
        if !sym.contains('.') && scope.get(sym).is_none() {
            // A concrete nullary alias retains information that an erased
            // descriptor cannot express (for example HCons.Self). Abstract
            // members retain their declaration identity in the fallback below.
            if args.is_empty() {
                if let Some(t) = self.self_type_member_at(st, bin, scope, sym, args, d, true) {
                    return Some(t);
                }
            }
            if let Some(t) = self.self_type_member(st, bin, scope, sym, args, d) {
                return Some(t);
            }
        }
        // `scala.package.List` and friends: package-object type aliases, which
        // pickles refer to by the alias, not the target. Expand through the
        // owner's own pickle rather than hard-coding a table. Tried before
        // `ensure_class` so an alias is not mistaken for a class of its own.
        let internal = sym.replace('.', "/");
        if crate::classpath::find_by_jvm(st, &internal).is_none() {
            if let Some(expanded) = self.expand_alias(st, bin, scope, sym, args, d, None) {
                return Some(expanded);
            }
        }
        let Some(cls) = self.ensure_class(st, bin, sym, false) else {
            // `scala.reflect.api.Trees.Tree` names an *abstract type member* of
            // trait `Trees`, not a class: there is no such classfile and never
            // will be. Almost the whole reflection API is written this way
            // (`Tree`, `Name`, `Symbol`, `Type`, `Position`, `FlagSet`), so
            // declining here would make the API unusable.
            //
            // The same holds for a *parameterised* one written out in full:
            // `RelationalProfile.API`'s `type BaseColumnType[T] =
            // RelationalTypesComponent.BaseColumnType[T]` is what
            // `import profile.api.*` offers, and its right-hand side names the
            // cake's abstract member applied to `T`.
            if let Some(t) = self.abstract_type_member(st, bin, sym, d) {
                if args.is_empty() {
                    return Some(t);
                }
                if st.kind_arity(&t) == args.len() {
                    let a = self.conv_all(st, bin, scope, args, d)?;
                    return Some(Type::Applied {
                        ctor: Box::new(t),
                        args: a,
                    });
                }
            }
            // Remembered rather than merely declined: the typer can often load
            // the classfile itself (any package on `-cp`, not just `scala.*`)
            // and ask again.
            if !self.unresolved_refs.iter().any(|s| s == sym) {
                self.unresolved_refs.push(sym.to_string());
            }
            return None;
        };
        // Each argument is converted knowing the *kind* its position wants, so
        // an unapplied class reference is accepted exactly where a
        // higher-kinded parameter stands and nowhere else.
        let tps = st.get(cls).tparams.clone();
        let mut a = Vec::with_capacity(args.len());
        for (i, arg) in args.iter().enumerate() {
            let want = tps.get(i).map(|t| st.get(*t).tparams.len()).unwrap_or(0);
            let conv = match arg {
                SigType::Ref {
                    sym: asym,
                    args: aargs,
                } => self.conv_ref(st, bin, scope, asym, aargs, d, want),
                other => self.conv_at(st, bin, scope, other, d),
            };
            a.push(conv?);
        }
        let arity = tps.len();
        if a.len() != arity {
            // A *wholly* unapplied reference to a parameterised class is a type
            // constructor, not an arity error -- but only where the position
            // asked for one: `implicit def asyncForIO: Async[IO]` pickles its
            // argument as a bare `IO`, because `Async`'s parameter is itself
            // `F[_]`. Declining it left every such witness unusable, and with
            // it the whole implicit scope of `Async[IO]`. Accepting it
            // *everywhere* was worse: a bare `Iterable` in an ordinary
            // position became a memberless `Iterable` that shadowed the real
            // `map`, and slick lost 99 more calls than it gained.
            if !a.is_empty() || arity == 0 || want_arity != arity {
                trace(format_args!(
                    "{sym}: applied to {} arguments but the symbol has {arity}",
                    a.len(),
                ));
                return None;
            }
        }
        Some(Type::Class { sym: cls, args: a })
    }

    /// Convert `p1#Shape.Packed` using the exact formal path retained by the
    /// pickle reader. The static class owner is checked independently from
    /// the term path, so two packages with a class named `Shape` cannot alias
    /// one another accidentally.
    fn param_projection(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        param_path: &str,
        member: &str,
    ) -> Option<Type> {
        let (param, param_sym) = self.param_singleton(param_path)?;
        let cls = st.class_sym_of(&param)?;
        let (owner_path, alias_name) = match member.rsplit_once('.') {
            Some((owner, name)) => (Some(owner), name),
            None => (None, member),
        };
        self.ensure_parents(st, bin, cls);
        if owner_path.is_some_and(|owner| {
            !inherits_matching(st, cls, |base| same_class_owner(st, base, owner))
        }) {
            return None;
        }
        self.complete_type_member(st, bin, cls, alias_name)?;
        let decl = self.completed_type_member_decl(cls, alias_name)?;
        let body = if st.get(decl).is_type_alias {
            st.get(decl).ty.clone()
        } else {
            Type::TypeMember(decl)
        };
        let body = st.subst_as_seen_from(&param, &body);
        let resolved = st.expand_in_type(&param, &body);
        if resolved != Type::TypeMember(decl) {
            return Some(resolved);
        }
        let projected = st.path_member(&[param_sym], decl, &param);
        Some(Type::TypeMember(projected))
    }

    fn conv_all(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        scope: &HashMap<String, Type>,
        args: &[SigType],
        d: u32,
    ) -> Option<Vec<Type>> {
        args.iter()
            .map(|a| self.conv_at(st, bin, scope, a, d))
            .collect()
    }

    /// `T`, or `T[args]`, as a type member of the class whose member is being
    /// completed, or of anything it inherits from.
    ///
    /// `None` outside a completion, and for any name no ancestor declares --
    /// a bare `Ref` is far more often a type parameter or a class, and this
    /// runs only after both of those have been ruled out.
    /// `p#T` read as the type it stands for.
    ///
    /// `p` is a type parameter or an abstract type member; resolving it to the
    /// class the surrounding cake fixes it to is what makes `T` more than its
    /// own declaration. `None` when nothing here can settle the prefix, in
    /// which case the caller falls back to `T`'s declaration -- the answer the
    /// prefix was dropped for before this existed.
    /// The symbol a projection prefix names when it is still abstract -- a
    /// type parameter of the class or member being read, or a type member
    /// left deferred -- and `None` when it is anything a reduction could use.
    fn abstract_prefix_sym(
        st: &SymbolTable,
        scope: &HashMap<String, Type>,
        prefix: &str,
    ) -> Option<SymbolId> {
        match scope.get(prefix)? {
            Type::TypeParam(id) => Some(*id),
            Type::TypeMember(id) if st.is_deferred_type_member(*id) => Some(*id),
            _ => None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn conv_projection(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        scope: &HashMap<String, Type>,
        prefix: &str,
        member: &str,
        args: &[SigType],
        d: u32,
    ) -> Option<Type> {
        if d > 24 {
            return None;
        }
        let short = member.rsplit_once('.').map(|(_, m)| m).unwrap_or(member);
        let mut stable_path = None;
        let pre = match scope.get(prefix) {
            Some(t) => t.clone(),
            None if prefix.ends_with(".type") => {
                let path = prefix.strip_suffix(".type")?;
                if let Some(module) = self.ensure_class(st, bin, path, true) {
                    Type::ModuleRef(module)
                } else {
                    // A singleton prefix may name a stable value rather than
                    // an object: `Owner.value.type#Owner.Member`. Resolve the
                    // accessor on Owner's module and retain its term path so
                    // an abstract Member remains path-dependent.
                    let (owner_name, value_name) = path.rsplit_once('.')?;
                    let owner = self.ensure_class(st, bin, owner_name, true)?;
                    self.adopt_binary_class(st, bin, owner);
                    let mut values = st.lookup_member(owner, value_name);
                    if values.is_empty() {
                        values = self.complete(st, bin, owner, value_name);
                    }
                    let value = values
                        .into_iter()
                        .find(|id| matches!(st.get(*id).kind, SymKind::Term | SymKind::Method))?;
                    stable_path = Some(vec![owner, value]);
                    Type::SingleType {
                        prefix: Box::new(Type::ModuleRef(owner)),
                        sym: value,
                    }
                }
            }
            None => {
                // The prefix is written in the vocabulary of the class the
                // member was asked of, not of the class that declares it.
                let root = self.completing_for.map(|c| Type::Class {
                    sym: c,
                    args: Vec::new(),
                });
                let outer = match root {
                    Some(r) => self.self_ty.replace(r),
                    None => self.self_ty.clone(),
                };
                let found = self
                    .self_type_member_at(st, bin, scope, prefix, &[], d + 1, true)
                    .or_else(|| self.stable_prefix_type(st, bin, prefix));
                trace(format_args!(
                    "projection prefix {prefix} at {:?}: {}",
                    self.self_ty.as_ref().map(|t| st.display_type(t)),
                    found
                        .as_ref()
                        .map(|t| st.display_type(t))
                        .unwrap_or_else(|| "-".into())
                ));
                self.self_ty = outer;
                found?
            }
        };
        // A prefix that is itself still abstract settles nothing.
        if matches!(&pre, Type::TypeMember(id) if st.is_deferred_type_member(*id)) {
            return None;
        }
        let cls = st.class_sym_of(&pre)?;
        let t = self.complete_type_member(st, bin, cls, short)?;
        // The prefix settled nothing: `T` came back as the same declaration
        // the fall-back path would have installed anyway.
        if let Type::TypeMember(id) = &t {
            if st.is_deferred_type_member(*id) {
                if let Some(path) = stable_path.as_deref() {
                    return Some(Type::TypeMember(st.path_member(path, *id, &pre)));
                }
                // An abstract member keeps its declaration identity, but its
                // stable module prefix still contributes to implicit scope.
                if matches!(pre, Type::ModuleRef(_)) {
                    let owners = st.type_member_prefixes.entry(id.0).or_default();
                    if !owners.contains(&cls) {
                        owners.push(cls);
                    }
                }
                return None;
            }
        }
        if args.is_empty() {
            // A projection uses the widened prefix, even when its class has
            // no type parameters. Substituting the class's this.type here
            // would turn Backend#Database into a different, stable path.
            return Some(st.subst_as_seen_from_at(&pre, Some(&pre), &st.dealias(&t)));
        }
        if st.kind_arity(&t) != args.len() {
            return None;
        }
        let a = self.conv_all(st, bin, scope, args, d)?;
        let applied = st.expand_applied_hk_alias(crate::symbol::apply_type_ctor(t, a));
        Some(st.subst_as_seen_from_at(&pre, Some(&pre), &applied))
    }

    /// The type of a stable *value* standing as a projection prefix
    /// (`backend.DatabaseFactory`), read off the class the member is being
    /// completed for. `self.self_ty` already names that class.
    fn stable_prefix_type(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        name: &str,
    ) -> Option<Type> {
        let Some(Type::Class { sym, .. }) = self.self_ty.clone() else {
            return None;
        };
        for owner in st.enclosing_classes(sym) {
            if !st.get(owner).is_class_like() {
                continue;
            }
            let mut found = st.lookup_member(owner, name);
            if found.is_empty() {
                found = self.complete(st, bin, owner, name);
            }
            for m in found {
                if !matches!(st.get(m).kind, SymKind::Term | SymKind::Method) {
                    continue;
                }
                let t = st.get(m).ty.result().clone();
                if st.class_sym_of(&t).is_some() {
                    return Some(t);
                }
            }
        }
        None
    }

    fn self_type_member(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        scope: &HashMap<String, Type>,
        name: &str,
        args: &[SigType],
        d: u32,
    ) -> Option<Type> {
        self.self_type_member_at(st, bin, scope, name, args, d, false)
    }

    /// `aliases_only` takes only a member some class *fixes*, when reading
    /// the *prefix* of a projection: a prefix is not a member, so no erased
    /// descriptor competes with the answer, and a prefix that is still
    /// abstract settles nothing -- taking it would stop the walk one class
    /// short of the alias that does.
    #[allow(clippy::too_many_arguments)]
    fn self_type_member_at(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        scope: &HashMap<String, Type>,
        name: &str,
        args: &[SigType],
        d: u32,
        aliases_only: bool,
    ) -> Option<Type> {
        let cls = match &self.self_ty {
            Some(Type::Class { sym, .. }) => *sym,
            _ => return None,
        };
        let internal = st.get(cls).jvm_name.clone();
        // Bare abstract members retain their declaration identity. Their
        // concrete definition is selected later at the receiver (including
        // refinements such as Witness { type T = A }); an erased descriptor
        // would irreversibly lose that information.
        if internal.is_empty()
            || internal.starts_with("java/")
            || internal.starts_with("javax/")
            || internal.contains("$anon")
        {
            return None;
        }
        let module = internal.ends_with('$');
        // The receiver, then each class it is nested in: `ConstantExtractor`
        // is declared inside trait `Constants`, and its `apply` returns that
        // trait's `Constant`, not one of its own.
        let mut owners: Vec<String> = Vec::new();
        let mut cur = internal.trim_end_matches('$').replace('/', ".");
        loop {
            owners.push(scala_rs_pickle::names::nested_to_dotted(&cur));
            match scala_rs_pickle::names::last_nesting_separator(&cur) {
                Some(i) => cur.truncate(i),
                None => break,
            }
        }
        for owner in owners {
            let lin = {
                let mut src = BinSource(bin);
                let mut errs = Vec::new();
                self.sigs.linearization(&mut src, &owner, module, &mut errs)
            };
            for step in lin {
                let qualified = format!("{}.{name}", step.class_name);
                // A *more derived* class in the linearisation may define the
                // same name as a concrete alias, and that definition is the
                // one the member's signature means. `scala.reflect.api
                // .Mirrors` declares `type RuntimeClass >: Null <: AnyRef`
                // and `scala.reflect.api.JavaUniverse` -- which comes first
                // here -- refines it to `type RuntimeClass = java.lang
                // .Class[_]`. Looking only for the abstract declaration
                // walked past the alias and installed the opaque one, so
                // `runtimeMirror(cl).classSymbol(classOf[A])` was "no
                // matching overload for (Mirrors.RuntimeClass)Symbols
                // .ClassSymbol with arguments (Class[A])" -- the parameter
                // type of a method whose classfile really takes a `Class`.
                // The linearisation is most-derived-first, so asking each
                // step for an alias before an abstract member is nsc's own
                // rule (a concrete definition overrides a deferred one).
                if !self.decl_site_erasure {
                    let outer = self.self_ty.take();
                    let alias =
                        self.expand_alias(st, bin, scope, &qualified, args, d, Some(&step.subst));
                    self.self_ty = outer;
                    if let Some(t) = alias {
                        trace(format_args!(
                            "{qualified}: concrete alias for a type member"
                        ));
                        return Some(t);
                    }
                }
                if aliases_only {
                    continue;
                }
                if let Some(t) = self.abstract_type_member(st, bin, &qualified, d) {
                    if args.is_empty() {
                        return Some(t);
                    }
                    // `BaseColumnType[Boolean]`: the member's own bound is
                    // written in its parameters, and `is_sub_type` substitutes
                    // them at each `Applied`, so the arity has to match.
                    if st.kind_arity(&t) != args.len() {
                        continue;
                    }
                    let a = self.conv_all(st, bin, scope, args, d)?;
                    return Some(Type::Applied {
                        ctor: Box::new(t),
                        args: a,
                    });
                }
            }
        }
        None
    }

    /// Resolve `Owner.T` where `T` is an **abstract type member** of `Owner`
    /// (`type Tree >: Null <: TreeApi`), installing it as a `TypeMember`
    /// symbol on `Owner` so the typer can see it through a path.
    ///
    /// Only abstract members are handled here; `type T = U` already goes
    /// through `expand_alias`, which substitutes the right-hand side.
    ///
    /// The upper bound is converted too, and a member whose bound cannot be
    /// converted is installed *unbounded* rather than declined: an opaque
    /// `u.Tree` that can be named and passed around is right, and losing the
    /// bound only means fewer members are reachable through it, never a wrong
    /// one.
    fn abstract_type_member(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        sym: &str,
        d: u32,
    ) -> Option<Type> {
        let (owner_name, member) = sym.rsplit_once('.')?;
        if member.is_empty() || !member.starts_with(|c: char| c.is_ascii_uppercase()) {
            return None;
        }
        // A path through an object (`NonEmptyLazyList.Type`) stores its
        // abstract member on the module-class pickle, while an ordinary
        // class stores it on the class pickle. Try both namespaces so a
        // package alias can expand through either spelling.
        let mut found = None;
        for module in [false, true] {
            let Some(owner) = self.ensure_class(st, bin, owner_name, module) else {
                continue;
            };
            // An ancestor's cached declaration cannot stand for this owner's
            // override: the latter may narrow an abstract member's bound.
            if let Some(id) = st
                .lookup_member(owner, member)
                .into_iter()
                .find(|&s| st.get(s).kind == SymKind::TypeMember && st.get(s).owner == owner)
            {
                return Some(Type::TypeMember(id));
            }
            let Ok(sig) = (|| {
                let mut src = BinSource(bin);
                self.sigs.class_sig(&mut src, owner_name, module)
            })() else {
                continue;
            };
            if let Some(m) = sig
                .members
                .iter()
                .find(|m| {
                    m.name == member
                        && m.kind == MemberKind::AbstractType
                        && m.flags & pflags::PARAM == 0
                })
                .cloned()
            {
                found = Some((owner, m));
                break;
            }
        }
        let (owner, m) = found?;
        let id = st.alloc(member, owner, SymKind::TypeMember, Flags::EMPTY, "");
        st.get_mut(owner).members.push(id);
        // A *parameterised* abstract member (`type BaseColumnType[T] <:
        // ColumnType[T] & BaseTypedType[T]`, slick's `RelationalTypesComponent`)
        // is pickled as a `PolyType` over the bounds. Reading only the bounds
        // dropped the parameters, so every use was "BaseColumnType does not
        // take type parameters" and the bound was left mentioning a `T`
        // nothing could stand for.
        let (tps, info) = match &m.ty {
            SigType::Poly { tparams, result } => (tparams.clone(), (**result).clone()),
            other => (Vec::new(), other.clone()),
        };
        let mut scope: HashMap<String, Type> = HashMap::new();
        let mut tparams = Vec::new();
        for tp in &tps {
            let t = st.alloc(&tp.name, id, SymKind::TypeParam, Flags::EMPTY, "");
            st.get_mut(t).ty = Type::TypeParam(t);
            set_tparam_kind(st, t, tp);
            scope.insert(tp.name.clone(), Type::TypeParam(t));
            tparams.push(t);
        }
        st.get_mut(id).tparams = tparams;
        // Entered before the bound is converted: `type Tree >: Null <: TreeApi`
        // and `type TreeApi` can refer to each other, and without the symbol
        // being visible first that recurses until the depth limit.
        if let SigType::Bounds { lo, hi } = &info {
            let (lo, hi) = (lo.clone(), hi.clone());
            // A bound is written in *its owner's* vocabulary, not the
            // receiver's. `type Ident >: Null <: IdentApi with RefTree` names
            // `RefTree` with a bare name, because it is another abstract type
            // member of the same trait `Trees`; resolving it against whatever
            // member happened to be under completion finds nothing when that
            // member lives elsewhere (`Internals.SyntacticTermIdentExtractor`).
            // The bound then loses `RefTree`, and with it the only path from
            // `Ident` to `Tree` -- so nothing built out of an `Ident`
            // typechecks. Point `self_ty` at the owner for the conversion.
            let outer = self.self_ty.replace(Type::Class {
                sym: owner,
                args: Vec::new(),
            });
            if let Some(h) = self.conv_upper_bound(st, bin, &scope, &hi, d) {
                if !matches!(h, Type::Any | Type::AnyRef) {
                    st.get_mut(id).bound_hi = Some(h);
                }
            }
            if let Some(l) = self.conv_at(st, bin, &scope, &lo, d) {
                if !matches!(l, Type::Nothing) {
                    st.get_mut(id).bound_lo = Some(l);
                }
            }
            self.self_ty = outer;
        }
        trace(format_args!("abstract type member {sym}"));
        Some(Type::TypeMember(id))
    }

    /// The upper bound of an abstract type member.
    ///
    /// Same as `conv_at`, except that a **compound** bound is kept rather than
    /// declined. The reflect API is written entirely in these:
    /// `type Select >: Null <: SelectApi with RefTree`, and `RefTree` in turn
    /// leads to `Tree`. Dropping the bound because `conv_at` has no answer for
    /// a refinement leaves `Select` unrelated to `Tree`, and then nothing built
    /// out of `Tree`s -- every `Syntactic*` call reification would emit --
    /// typechecks. Declarations inside the refinement are dropped: only the
    /// parents decide conformance, and inventing members here would be a
    /// guess. A parent that does not convert is skipped rather than failing the
    /// bound, on the same principle as the caller's: fewer members reachable
    /// through the type is right, a wrong one is not.
    fn conv_upper_bound(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        scope: &HashMap<String, Type>,
        hi: &SigType,
        d: u32,
    ) -> Option<Type> {
        let SigType::Refined { parents, .. } = hi else {
            return self.conv_at(st, bin, scope, hi, d);
        };
        let mut ps: Vec<Type> = Vec::new();
        for p in parents {
            let Some(t) = self.conv_at(st, bin, scope, p, d) else {
                continue;
            };
            if matches!(t, Type::Any | Type::AnyRef) || ps.contains(&t) {
                continue;
            }
            ps.push(t);
        }
        match ps.len() {
            0 => None,
            1 => Some(ps.remove(0)),
            _ => Some(Type::Refined {
                parents: ps,
                decls: Vec::new(),
            }),
        }
    }

    /// Resolve `owner.Name[args]` where `Name` is a type alias declared on
    /// `owner` (typically a package object), by reading `owner`'s pickle and
    /// substituting the alias's own type parameters.
    #[allow(clippy::too_many_arguments)]
    fn expand_alias(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        scope: &HashMap<String, Type>,
        sym: &str,
        args: &[SigType],
        d: u32,
        owner_subst: Option<&HashMap<String, SigType>>,
    ) -> Option<Type> {
        let (owner, simple) = sym.rsplit_once('.')?;
        // Type references use encoded names, while signature declarations
        // use source names (for example HList.$colon$colon versus `::`).
        let simple = scala_rs_pickle::names::decode_method_name(simple);
        // A companion's signature may exist without declaring this alias.
        // Continue to the class signature instead of treating that as a hit.
        let direct = [true, false].into_iter().find_map(|module| {
            let mut src = BinSource(bin);
            let sig = self.sigs.class_sig(&mut src, owner, module).ok()?;
            let alias = sig
                .members_named(&simple)
                .find(|m| m.kind == MemberKind::TypeAlias)
                .cloned();
            alias.map(|alias| (owner.to_string(), module, alias))
        });
        // A reference may name an alias through its package, whose scope
        // includes the package object's members. Keep inherited aliases and
        // their parent substitutions, just as importing the package does.
        let (resolved_owner, module, mut alias) = direct.or_else(|| {
            let package_object = format!("{owner}.package");
            let (hits, _) = self
                .sigs
                .lookup(&mut BinSource(bin), &package_object, true, &simple);
            hits.into_iter()
                .find(|hit| hit.member.kind == MemberKind::TypeAlias && hit.member.is_public_api())
                .map(|hit| (package_object, true, hit.member))
        })?;
        let owner = resolved_owner.as_str();
        // A nested receiver can inherit an alias from a parameterized outer
        // parent: C extends Base[String], Base[A] { type B = A }. Substitute
        // the parent's A before applying alias arguments in the caller's scope.
        let inferred_subst = if owner_subst.is_none() {
            self.alias_owner_subst(st, bin, owner)
        } else {
            None
        };
        if let Some(subst) = owner_subst.or(inferred_subst.as_ref()) {
            alias.ty = scala_rs_pickle::sym::apply_subst(&alias.ty, subst);
        }
        // A parameterised alias (`type List[+A] = immutable.List[A]`) binds its
        // own parameters to our arguments; a plain one has none.
        let (tps, target) = match &alias.ty {
            SigType::Poly { tparams, result } => (tparams.clone(), (**result).clone()),
            other => (Vec::new(), other.clone()),
        };
        if tps.len() != args.len() {
            // A parameterised alias named with *no* arguments is a type
            // constructor: `Query[+E, U, C[_]]`'s third argument is pickled
            // as a bare `scala.package.Seq`, and `type Seq[+A] =
            // scala.collection.immutable.Seq[A]` is an alias. Declining it
            // failed the whole parent, which is why slick's `TableQuery`
            // never got a pickled `Query` parent at all. A plain eta-expansion
            // can use the target constructor directly; other aliases need
            // their own type parameters and right-hand side retained.
            if args.is_empty() && !tps.is_empty() {
                if let SigType::Ref {
                    sym: target,
                    args: targs,
                } = &target
                {
                    if targs.len() == tps.len()
                    && targs.iter().zip(tps.iter()).all(|(a, tp)| {
                        matches!(a, SigType::Ref { sym, args } if args.is_empty() && *sym == tp.name)
                    })
                    {
                        return self.conv_ref(st, bin, scope, target, &[], d, tps.len());
                    }
                }
                // Non-eta aliases still have a constructor: retain their
                // parameters and RHS instead of confusing a companion object
                // with the type (for example `type Id[A] = A; object Id`).
                let owner = self.ensure_class(st, bin, owner, module)?;
                return self.install_type_alias(st, bin, owner, &simple, &alias.ty, None);
            }
            return None;
        }
        let mut map: HashMap<String, SigType> = HashMap::new();
        let mut alias_scope = scope.clone();
        let mut target_refs = Vec::new();
        walk(&target, &mut target_refs, 0);
        for (tp, a) in tps.iter().zip(args.iter()) {
            map.insert(tp.name.clone(), a.clone());
            // A projection can carry its prefix inside the reference name
            // (`T#Owner.Member`). Signature substitution replaces ordinary
            // references to T, but cannot replace that embedded prefix. Keep
            // the applied alias's binders in the conversion scope as well.
            //
            // Only an argument that names no type parameter of the caller's
            // scope, though. A projection through a type parameter can only
            // be read through the parameter's bound, and binding that reading
            // here fixes it before the parameter is ever instantiated. slick's
            // `type Apply[N <: Nat] = HeadOf[Drop[N]]` with `type HeadOf[T <:
            // HList] = T#Head` bound `T` to `Nat.Fold[HList, TailOf, Self]`,
            // `N` gone, and `hlist(1)` on an `Int :: String :: HNil` came out
            // as the `Int` at index 0 (ClassCastException at run time).
            let prefix = format!("{}#", tp.name);
            let names_scope_tparam = || {
                let mut arg_refs = Vec::new();
                walk(a, &mut arg_refs, 0);
                arg_refs.iter().any(|r| {
                    let head = r.split('#').next().unwrap_or(r);
                    matches!(scope.get(head), Some(Type::TypeParam(_)))
                })
            };
            if target_refs.iter().any(|r| r.starts_with(&prefix)) && !names_scope_tparam() {
                if let Some(bound) = self.conv_at(st, bin, scope, a, d) {
                    alias_scope.insert(tp.name.clone(), bound);
                }
            }
        }
        let target = scala_rs_pickle::sym::apply_subst(&target, &map);
        // The substituted arguments are still written in the caller's
        // vocabulary, so the caller's scope is what finishes the job.
        self.conv_at(st, bin, &alias_scope, &target, d)
    }

    /// A qualified alias can name an ancestor of an enclosing class. Keep
    /// that ancestor's arguments when the alias appears in a method bound.
    fn alias_owner_subst(
        &mut self,
        st: &SymbolTable,
        bin: &mut BinaryIndex,
        alias_owner: &str,
    ) -> Option<HashMap<String, SigType>> {
        let receiver = self.self_ty.as_ref()?;
        let cls = st.class_sym_of(receiver)?;
        let internal = &st.get(cls).jvm_name;
        let module = internal.ends_with('$');
        let mut name = internal.trim_end_matches('$').replace('/', ".");
        loop {
            let owner = scala_rs_pickle::names::nested_to_dotted(&name);
            let mut src = BinSource(bin);
            let mut errors = Vec::new();
            for step in self
                .sigs
                .linearization(&mut src, &owner, module, &mut errors)
            {
                if step.class_name == alias_owner {
                    return Some(step.subst);
                }
            }
            name.truncate(scala_rs_pickle::names::last_nesting_separator(&name)?);
        }
    }
}

/// `f$default$2` names the getter for `f`'s second parameter's default.
fn is_default_getter(name: &str) -> bool {
    let Some((_, n)) = name.rsplit_once("$default$") else {
        return false;
    };
    !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())
}

/// Synthetic implicit-class conversions are part of a library's source API.
/// Preserve compiler-owned prelude declarations and the two lazy conversion
/// models whose lowering is handled directly by the typer. A package beginning
/// with `scala` is not by itself evidence that a library has a compiler model.
fn implicit_class_conversion_from(
    st: &SymbolTable,
    class_sym: SymbolId,
    owner: &str,
    m: &scala_rs_pickle::Member,
) -> bool {
    if !m.is_implicit_class_conversion() || class_sym.0 < st.prelude_end {
        return false;
    }
    let owner = owner.replace('/', ".").replace('$', ".");
    let duration = owner.trim_end_matches('.') == "scala.concurrent.duration.package"
        && matches!(
            m.name.as_str(),
            "DurationInt" | "DurationLong" | "DurationDouble"
        );
    let quasiquote =
        owner.trim_end_matches('.') == "scala.reflect.api.Quasiquotes" && m.name == "Quasiquote";
    !duration && !quasiquote
}

/// The unspecialized class a `@specialized` variant was generated from.
///
/// scalac's specialization phase runs *after* pickling, so `Foo$mcJ$sp` and
/// friends exist only in the class files; no pickle mentions them, and nsc's
/// own reader never sees one because it takes a Scala class's shape from the
/// `ScalaSignature`. We read the class file's `Signature` attribute instead,
/// where the superclass of slick's `LongJdbcType` really is
/// `JdbcTypesComponent$DriverJdbcType$mcJ$sp` -- whose own superclass is
/// `DriverJdbcType<Object>`, since a specialized subclass fixes the parameter
/// it specializes on. Recognising the suffix lets the pickled parent replace
/// it rather than sit next to it.
///
/// The suffix is `$mc` followed by one specialization tag per type parameter
/// (`Z B C D F I J S V`, the JVM primitive letters plus `V`) and then `$sp`.
fn despecialized(jvm_name: &str) -> Option<&str> {
    let rest = jvm_name.strip_suffix("$sp")?;
    let at = rest.rfind("$mc")?;
    let tags = &rest[at + 3..];
    if tags.is_empty() || !tags.bytes().all(|b| b"ZBCDFIJSV".contains(&b)) {
        return None;
    }
    Some(&rest[..at])
}

/// Drop the class-file descriptions of a name the pickle has just replaced.
///
/// `stale` is what the class-file reader had entered under that name and
/// `installed` is what [`PickleSupply::complete_named`] has just supplied.
/// Those two lists can *overlap*: supplying a member fills the pickled
/// signature into a symbol that is already there rather than allocating a
/// second one, and it then pushes that same symbol onto the class again. A
/// plain "remove everything stale" therefore deletes the member it was
/// supposed to install, and a plain "remove nothing" leaves it listed twice,
/// where every selection on it is an ambiguous overload against itself.
///
/// blocking-slick's `queryToQueryInvoker` and its ten siblings are the case,
/// and only under `import profile.blockingApi._` -- the one order in which the
/// class file is read before the pickle is adopted. `q.list` / `q.update(…)`
/// were "value list is not a member of Query[…]" there and compiled anywhere
/// else.
fn drop_stale_members(
    st: &mut SymbolTable,
    class_sym: SymbolId,
    stale: &[SymbolId],
    installed: &[SymbolId],
) {
    let mut kept: HashSet<SymbolId> = HashSet::new();
    st.get_mut(class_sym).members.retain(|m| {
        if !installed.contains(m) {
            return !stale.contains(m);
        }
        kept.insert(*m)
    });
}

/// The type arguments of a class type, empty for anything else.
fn parent_args(t: &Type) -> Vec<Type> {
    match t {
        Type::Class { args, .. } => args.clone(),
        _ => Vec::new(),
    }
}

/// Whether `cls` already has `target` somewhere above it.
pub(crate) fn inherits_from(st: &SymbolTable, cls: SymbolId, target: SymbolId) -> bool {
    inherits_matching(st, cls, |c| c == target)
}

/// Whether `cls` or a reachable parent matches a predicate. Keep the same
/// order and traversal bound as `inherits_from`, including checking a node
/// before the visited/budget guard.
pub(crate) fn inherits_matching(
    st: &SymbolTable,
    cls: SymbolId,
    mut matches: impl FnMut(SymbolId) -> bool,
) -> bool {
    let mut seen: Vec<u32> = Vec::new();
    let mut work = vec![cls];
    while let Some(c) = work.pop() {
        if matches(c) {
            return true;
        }
        if seen.contains(&c.0) || seen.len() > 256 {
            continue;
        }
        seen.push(c.0);
        for p in &st.get(c).parents {
            if let Some(ps) = st.class_sym_of(p) {
                work.push(ps);
            }
        }
    }
    false
}

/// Compare a pickled owner path with the exact class identity, including its
/// package and nested-class spelling. A simple-name comparison lets unrelated
/// `one.Shape` and `two.Shape` satisfy the same dependent projection.
fn same_class_owner(st: &SymbolTable, cls: SymbolId, owner: &str) -> bool {
    let internal = st.get(cls).jvm_name.clone();
    if internal.is_empty() {
        return false;
    }
    let dotted = internal.trim_end_matches('$').replace('/', ".");
    owner == dotted || owner == dotted.replace('$', ".")
}

/// The JVM descriptor a converted parameter type erases to, where that is
/// certain. `None` means "some reference type" -- a type parameter, `Any`, or
/// anything else that erases to `Object` or to a class we cannot pin down --
/// and matches any reference slot.
/// Whether the class file `candidate` (a JVM internal name) is the one that
/// *declares* `full_name`, rather than merely an enclosing class whose file
/// carries its pickle.
///
/// `a/b/Outer$Inner` names `a.b.Outer.Inner`; `a/b/Outer` does not. Trailing
/// `$` (a module class) does not change the simple name.
/// The class symbols a class's parent list names.
fn parent_classes(st: &SymbolTable, cls: SymbolId) -> Vec<SymbolId> {
    st.get(cls)
        .parents
        .iter()
        .filter_map(|p| match p {
            Type::Class { sym, .. } => Some(*sym),
            _ => None,
        })
        .collect()
}

/// Whether a JVM internal name names a class nested inside another class.
///
/// `scala/collection/Iterator$GroupedIterator` yes; `scala/collection/Iterator`
/// and the module class `scala/Predef$` no.
fn is_nested_jvm_name(internal: &str) -> bool {
    internal
        .rsplit('/')
        .next()
        .is_some_and(|simple| simple.trim_end_matches('$').contains('$'))
}

fn names_class(candidate: &str, full_name: &str) -> bool {
    let Some(simple) = full_name.rsplit('.').next() else {
        return false;
    };
    let last = candidate.rsplit('/').next().unwrap_or(candidate);
    let last = last.strip_suffix('$').unwrap_or(last);
    let start = scala_rs_pickle::names::last_nesting_separator(last).map_or(0, |i| i + 1);
    &last[start..] == simple
}

/// The source-style names to try for a pickled singleton reference.
///
/// Most `Single` references use a dotted Scala name, but some nsc pickles
/// carry a binary nested name in the final segment while retaining the
/// enclosing source path.  `ColumnOption.PrimaryKey` is the representative
/// shape: `ColumnOption.ColumnOption$PrimaryKey` must be looked up as
/// `ColumnOption.PrimaryKey`.  Keep the original first so a legitimate class
/// whose source name contains `$` retains its existing interpretation.
fn singleton_type_names(sym: &str) -> Vec<String> {
    let mut names = vec![sym.to_string()];
    let Some((prefix, last)) = sym.rsplit_once('.') else {
        return names;
    };
    let mut nested = last.split('$');
    let Some(root) = nested.next() else {
        return names;
    };
    let tail: Vec<&str> = nested.filter(|part| !part.is_empty()).collect();
    if tail.is_empty() {
        return names;
    }
    let prefix_parts: Vec<&str> = prefix.split('.').collect();
    let Some(root_index) = prefix_parts.iter().rposition(|part| *part == root) else {
        return names;
    };
    let mut source_parts = prefix_parts[..root_index].to_vec();
    source_parts.push(root);
    source_parts.extend(tail);
    let source = source_parts.join(".");
    if source != sym {
        names.push(source);
    }
    names
}

/// Every value parameter of a pickled member's type, across all its clauses.
fn sig_value_params(t: &SigType) -> Vec<scala_rs_pickle::sym::Param> {
    match t {
        SigType::Poly { result, .. }
        | SigType::Existential { result, .. }
        | SigType::Annotated(result) => sig_value_params(result),
        SigType::Method { params, result, .. } => {
            let mut v = params.clone();
            v.extend(sig_value_params(result));
            v
        }
        _ => Vec::new(),
    }
}

/// A member's parameters, erased the same way [`erased_param_desc`] erases a
/// freshly-read pickle shape, so an already-installed symbol's shape can be
/// compared against one about to be installed.
pub(crate) fn flat_erased_params(st: &SymbolTable, ty: &Type) -> Vec<Option<String>> {
    match ty {
        Type::Method { paramss, .. } => paramss
            .iter()
            .flatten()
            .map(|t| erased_param_desc(st, t))
            .collect(),
        _ => Vec::new(),
    }
}

fn erased_param_desc(st: &SymbolTable, ty: &Type) -> Option<String> {
    // An abstract type member (`Type::TypeMember`) has to resolve to its own
    // upper bound before it can name a fixed slot; the loop below re-runs
    // this match on that bound (which may itself be another abstract type
    // member, chained a few hops deeper) instead of recursing, with a
    // generous but finite cap against a cyclic bound.
    let mut cur = ty.clone();
    for _ in 0..16 {
        cur = match &cur {
            Type::Boolean => return Some("Z".into()),
            Type::Byte => return Some("B".into()),
            Type::Short => return Some("S".into()),
            Type::Char => return Some("C".into()),
            Type::Int => return Some("I".into()),
            Type::Long => return Some("J".into()),
            Type::Float => return Some("F".into()),
            Type::Double => return Some("D".into()),
            // `V` is legal only as a method return descriptor. In every value
            // position scalac boxes Unit, including an ordinary parameter
            // such as MUnit's implicit `unitToProp(unit: Unit)`.
            Type::Unit => return Some("Lscala/runtime/BoxedUnit;".into()),
            Type::String => return Some("Ljava/lang/String;".into()),
            Type::Array(_) => {
                // Array[A <: Int] is Object, while Array[Int] is int[].
                // Share the backend erasure rule before matching a binary
                // declaration; erasing the element alone loses this distinction.
                return match crate::erasure::erase_member_ty(&cur, st) {
                    Type::Array(elem) => erased_param_desc(st, &elem).map(|d| format!("[{d}")),
                    erased => erased_param_desc(st, &erased),
                };
            }
            // These Scala top types have a definite erased reference slot.
            // Leaving AnyVal unknown confuses it with String/NodeSeq overloads.
            Type::Any | Type::AnyRef | Type::AnyVal => return Some("Ljava/lang/Object;".into()),
            Type::Function { params, .. } => {
                return Some(format!("Lscala/Function{};", params.len()))
            }
            Type::ByName(_) => return Some("Lscala/Function0;".into()),
            // Scala 2.13 repeated parameters use immutable.Seq on the JVM.
            // Leaving this slot unknown drops Class* beside a same-arity
            // PartialFunction overload (Exception.catching and its siblings).
            Type::Repeated(_) => return Some("Lscala/collection/immutable/Seq;".into()),
            // A direct value-class parameter uses its underlying JVM slot.
            // Reference/generic containers still keep the boxed class type.
            Type::Class { sym, .. } if st.is_value_class(*sym) => {
                // A generic value class erases after substituting its actual
                // type arguments into the wrapped field. Reading the raw
                // field directly leaves `class Wrapper[A](val value: A)` at
                // `A` (or at Object for a classfile-only field), so a method
                // taking `Wrapper[Concrete]` cannot be matched to its real
                // JVM descriptor. Share the compiler's value-class erasure,
                // which performs that substitution before following the
                // wrapped type.
                let erased = crate::erasure::erase_member_ty(&cur, st);
                if erased == cur {
                    return None;
                }
                erased
            }
            Type::Class { sym, .. } => {
                let n = st.get(*sym).jvm_name.clone();
                return if n.is_empty() || n.starts_with('[') {
                    None
                } else {
                    Some(format!("L{n};"))
                };
            }
            // `type Symbol >: Null <: SymbolApi`, reached from the abstract
            // `scala.reflect.api.Trees`/`Universe` API rather than the
            // concrete `JavaUniverse` a macro only gets at actual expansion
            // time (`u.Ident(sym: Symbol)` -- `crates/cli/tests/quasi.rs`'s
            // `lf3_identsym_*`, slick's `TableQueryMacroImpl.apply`). nsc
            // itself erases an abstract type to its own upper bound
            // (`Object` with none), and
            // the classfile really does declare `Ident(LSymbolApi;)Lscala
            // /reflect/api/Trees$IdentApi;` at this level, verified with
            // `javap` on `scala.reflect.api.Trees` in scala-reflect.jar
            // 2.13.16. Without this, every abstract-type-member parameter
            // erased to `None` (a wildcard "any reference slot") instead, so
            // `Ident(String)` and `Ident(Symbol)` -- both one reference
            // parameter -- were indistinguishable at the "which classfile
            // method is this" step (`no unambiguous erased descriptor`), and
            // only the arbitrarily-first one of the two ever installed.
            Type::TypeMember(id) => match st.get(*id).bound_hi.clone() {
                Some(hi) => hi,
                None => return Some("Ljava/lang/Object;".into()),
            },
            // A type parameter is an unnamed reference slot -- except when
            // it is bounded by a primitive, which it then erases to
            // (`def id[A <: Int](a: A): A` is `id(I)I`; see
            // `erasure::bound_erasure`). Left unnamed, it matched no
            // descriptor at all and the member was never supplied.
            Type::TypeParam(id) => match st.get(*id).bound_hi.clone() {
                Some(
                    hi @ (Type::Boolean
                    | Type::Byte
                    | Type::Short
                    | Type::Char
                    | Type::Int
                    | Type::Long
                    | Type::Float
                    | Type::Double
                    | Type::TypeParam(_)),
                ) => hi,
                _ => return None,
            },
            _ => return None,
        };
    }
    None
}

fn erased_return_desc(st: &SymbolTable, ty: &Type) -> Option<String> {
    if matches!(ty, Type::Unit) {
        Some("V".into())
    } else {
        erased_param_desc(st, ty)
    }
}

/// Whether a candidate descriptor's parameters agree with the slots we could
/// name. An unnamed slot matches any reference parameter but not a primitive:
/// that is what separates `from(int)` from `from(IterableOnce)`.
fn params_match(desc: &str, want: &[Option<String>]) -> bool {
    let Some(got) = desc_params(desc) else {
        return false;
    };
    if got.len() != want.len() {
        return false;
    }
    got.iter().zip(want).all(|(g, w)| match w {
        Some(w) => g == w,
        None => g.starts_with('L') || g.starts_with('['),
    })
}

/// Split a method descriptor's parameter list into individual descriptors.
pub(crate) fn desc_params(desc: &str) -> Option<Vec<String>> {
    let b = desc.as_bytes();
    if b.first() != Some(&b'(') {
        return None;
    }
    let mut out = Vec::new();
    let mut i = 1;
    while i < b.len() && b[i] != b')' {
        let start = i;
        while i < b.len() && b[i] == b'[' {
            i += 1;
        }
        if i >= b.len() {
            return None;
        }
        if b[i] == b'L' {
            while i < b.len() && b[i] != b';' {
                i += 1;
            }
            if i >= b.len() {
                return None;
            }
        }
        i += 1;
        out.push(desc[start..i].to_string());
    }
    if i >= b.len() {
        return None;
    }
    Some(out)
}

/// Number of parameters in a JVM method descriptor.
pub(crate) fn desc_arity(desc: &str) -> Option<usize> {
    let b = desc.as_bytes();
    if b.first() != Some(&b'(') {
        return None;
    }
    let mut i = 1;
    let mut n = 0;
    while i < b.len() && b[i] != b')' {
        while i < b.len() && b[i] == b'[' {
            i += 1;
        }
        if i >= b.len() {
            return None;
        }
        if b[i] == b'L' {
            while i < b.len() && b[i] != b';' {
                i += 1;
            }
            if i >= b.len() {
                return None;
            }
        }
        i += 1;
        n += 1;
    }
    if i >= b.len() {
        return None;
    }
    Some(n)
}

/// The JVM descriptor of `class_sym`'s hidden enclosing-instance parameter.
/// The classpath symbol owner and the classfile's static-nested flag are both
/// required: a top-level class whose source name contains `$` is not an inner
/// class, while a class nested in an object is static and has no `$outer` slot.
/// Returning the descriptor also prevents an unrelated leading parameter from
/// being accepted merely because its arity happens to be one larger.
fn hidden_outer_desc(
    st: &SymbolTable,
    class_sym: SymbolId,
    classfile: &JavaClass,
) -> Option<String> {
    if class_sym.is_none()
        || classfile.nested_static
        || st.get(class_sym).flags.contains(Flags::STATIC)
    {
        return None;
    }
    // The JVM's `InnerClasses` owner is the lexical owner, but Scala can
    // lower a class nested in a component trait with a different enclosing
    // instance type.  For example Slick's
    // `RelationalTableComponent.Table` is lexically inside the component but
    // carries `$outer: RelationalProfile`, and both constructor descriptors
    // begin with that profile type.  The `$outer` field is the classfile's
    // exact witness for the hidden constructor slot; prefer it over deriving
    // a descriptor from the symbol owner.
    if let Some(outer) = classfile.outer_desc.as_deref() {
        return Some(outer.to_string());
    }
    let owner = st.get(class_sym).owner;
    if owner.is_none() || !st.get(owner).is_class_like() || owner == class_sym {
        return None;
    }
    let outer = st.get(owner).jvm_name.as_str();
    (!outer.is_empty()).then(|| format!("L{outer};"))
}

/// Whether an already-installed constructor has the source shape that a
/// pickle just resolved. A leading extra parameter is accepted only when the
/// class metadata proves this is a non-static inner class; arity alone must
/// never bind a non-inner overload to a source constructor.
fn ctor_params_match(
    st: &SymbolTable,
    ctor: SymbolId,
    want: &[Option<String>],
    hidden_outer: Option<&str>,
) -> bool {
    let got: Vec<Option<String>> = st
        .get(ctor)
        .params
        .iter()
        .map(|p| erased_param_desc(st, &st.get(*p).ty))
        .collect();
    let tail = if got.len() == want.len() {
        Some(got.as_slice())
    } else if hidden_outer.is_some_and(|outer| {
        got.len() == want.len() + 1 && got.first().and_then(Option::as_deref) == Some(outer)
    }) {
        Some(&got[1..])
    } else {
        None
    };
    tail.is_some_and(|tail| tail == want)
}

/// The access modifier a pickled `<init>` carries, as this compiler's flags.
///
/// A class file cannot answer this: nsc emits a `private` constructor
/// `ACC_PUBLIC`, so the `ScalaSignature` is the only record of it, and reading
/// it is what makes `neg/t6601` -- a *separate* compilation -- reject.
///
/// `private[p]` and `protected[p]` are pickled as the bare flag **plus** a
/// `privateWithin` reference, and the reader now resolves `p` to its simple
/// name (`Member::private_within`). The flag is therefore kept for those too,
/// and `install_ctor` copies the boundary alongside it so `Typer::accessible`
/// asks `access_within_of` rather than `nested_in`.
///
/// A boundary that did **not** resolve leaves `Flags::EMPTY`, as this did for
/// every qualified access before: `access_within_of` denies when it cannot
/// find the boundary, so a name we failed to read would refuse every call --
/// including the `private[slick]` constructors slick's own code makes -- and
/// over-rejection is the one failure mode this must not have.
fn ctor_access_flags(member: &scala_rs_pickle::sym::Member) -> Flags {
    // A boundary the pickle states but this reader could not name. Left as
    // accessible as it was before any of this existed: `access_within_of`
    // denies when it cannot find the boundary, so marking it `private` on the
    // strength of the flag alone would refuse every `private[slick]`
    // constructor slick itself calls.
    if member.has_private_within && member.private_within.is_none() {
        return Flags::EMPTY;
    }
    if member.has(pflags::PRIVATE) {
        Flags::PRIVATE
    } else if member.has(pflags::PROTECTED) {
        Flags::PROTECTED
    } else {
        Flags::EMPTY
    }
}

/// The access boundary a pickled `<init>` names, when the pickle resolved it.
///
/// Returned separately from the flag because the two travel to different
/// fields, and because a boundary that did not resolve must leave the
/// constructor exactly as accessible as it was: `access_within_of` **denies**
/// when it cannot find the boundary, so a name we failed to read would refuse
/// every call -- the `private[slick]` constructors slick's own code makes
/// included -- and over-rejection is the one failure mode this must not have.
fn ctor_access_within(member: &scala_rs_pickle::sym::Member) -> Option<String> {
    member.private_within.clone()
}

/// Does this constructor symbol carry a parameter whose type never resolved?
///
/// `Type::Named` is what `classpath::parse_desc` leaves behind when a class
/// named by a descriptor is not in the symbol table yet: it is not the class,
/// it conforms to nothing, and no argument ever matches it.
fn ctor_has_unresolved_param(st: &SymbolTable, ctor: SymbolId) -> bool {
    let unresolved = |t: &Type| matches!(t, Type::Named { .. } | Type::Error);
    let by_ty = match &st.get(ctor).ty {
        Type::Method { paramss, .. } => paramss.iter().flatten().any(unresolved),
        _ => false,
    };
    by_ty
        || st
            .get(ctor)
            .params
            .iter()
            .any(|p| unresolved(&st.get(*p).ty))
}

/// Give a `-cp` value class the single constructor field that *is* its
/// representation.
///
/// `SymbolTable::is_value_class` asks for an `AnyVal` parent **and** exactly
/// one constructor field, and `value_class_underlying` reads that field's type
/// to decide what the class erases to. The eager `-cp` installer builds the
/// field from the pickled constructor (`classpath::install_ctor`), but only
/// top-level class files carry a `ScalaSignature`: a *nested* value class --
/// `fs2.Stream.PartiallyAppliedFromIterator`, and every one of slick's -- is
/// adopted from its class file alone and so has no constructor field at all.
///
/// The class file does have it. nsc compiles the single `val` to one private
/// final instance field, so that field's descriptor is the underlying type.
/// Its name is expanded for a nested class
/// (`fs2$Stream$PartiallyAppliedFromIterator$$blocking`); the source name is
/// what follows the last `$$`, which is the name the accessor carries too.
///
/// The symbol is allocated ownerless and then re-owned, the way
/// `stub_nested_module` does it: entering it in the class's member list would
/// put a second `blocking` next to the accessor the pickle supplies, and
/// member lookup would have to choose between them.
fn ensure_value_class_field(
    st: &mut SymbolTable,
    bin: &mut BinaryIndex,
    class_sym: SymbolId,
    pickled_underlying: Option<Type>,
) {
    let internal = st.get(class_sym).jvm_name.clone();
    if internal.is_empty() {
        return;
    }
    let Ok(Some(bytes)) = bin.find_class(&internal) else {
        return;
    };
    let Ok(jc) = parse_java_classfile(&bytes) else {
        return;
    };
    let Some(f) = jc.sole_instance_field.clone() else {
        return;
    };
    // A private underlying accessor has an expanded JVM name. Keep the
    // constructor parameter's source spelling for named arguments, and read
    // the actual unboxing entry point from the classfile instead.
    if jc
        .methods
        .iter()
        .any(|m| m.name == f.name && m.desc == format!("(){}", f.desc))
    {
        st.record_value_class_getter(class_sym, f.name.clone());
    }
    if let Some(&field) = st.get(class_sym).ctor_fields.first() {
        if let Some(underlying) = pickled_underlying {
            st.get_mut(field).ty = underlying;
        }
        return;
    }
    let name = f.name.rsplit("$$").next().unwrap_or(&f.name).to_string();
    let ty =
        pickled_underlying.unwrap_or_else(|| crate::classpath::field_ty_from_desc(st, &f.desc));
    let fid = st.alloc(&name, SymbolId::NONE, SymKind::Term, Flags::EMPTY, "");
    st.get_mut(fid).owner = class_sym;
    st.get_mut(fid).ty = ty;
    st.get_mut(class_sym).ctor_fields = vec![fid];
    trace(format_args!(
        "{internal}: value class field {name}{}",
        f.desc
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TypecheckOptions;

    fn jar() -> Option<std::path::PathBuf> {
        let p = std::path::PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
        p.is_file().then_some(p)
    }

    fn library_opts(jar: &std::path::Path) -> TypecheckOptions {
        TypecheckOptions {
            library_abi: true,
            binary_path: vec![jar.to_path_buf()],
            ..TypecheckOptions::default()
        }
    }

    /// A member the prelude declares by hand must keep coming from the
    /// prelude: completion runs only after resolution failed, so it can add
    /// members but never replace one. A supplied member is recognisable by the
    /// JVM descriptor parked in its `jvm_name`.
    #[test]
    fn the_prelude_wins_over_the_pickle() {
        let Some(jar) = jar() else {
            eprintln!("skip: scala-library jar not present");
            return;
        };
        let src = r#"
object Main {
  def main(args: Array[String]): Unit = {
    val xs = List(1, 2, 3)
    xs.map(x => x + 1)
    xs.tails
  }
}
"#;
        let (_t, st, diags) = crate::typecheck_str_opts(src, &library_opts(&jar));
        assert!(
            !crate::has_errors(&diags),
            "type errors: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        let list = st.list_sym;
        let maps: Vec<_> = st
            .get(list)
            .members
            .iter()
            .filter(|&&m| st.get(m).name == "map")
            .collect();
        assert_eq!(
            maps.len(),
            1,
            "prelude List#map was duplicated by the pickle"
        );
        assert!(
            st.get(*maps[0]).jvm_name.is_empty(),
            "prelude List#map was replaced by a pickle-supplied one"
        );
        // ...while a member the prelude does not have does come from the pickle.
        let tails: Vec<_> = st
            .get(list)
            .members
            .iter()
            .filter(|&&m| st.get(m).name == "tails")
            .collect();
        assert_eq!(tails.len(), 1, "expected exactly one supplied tails");
        assert!(
            st.get(*tails[0]).jvm_name.starts_with('('),
            "List#tails should carry an erased descriptor, got {:?}",
            st.get(*tails[0]).jvm_name
        );
    }

    /// Nothing is read until a lookup actually misses.
    #[test]
    fn nothing_is_supplied_when_nothing_is_missing() {
        let Some(jar) = jar() else {
            eprintln!("skip: scala-library jar not present");
            return;
        };
        let src = r#"
object Main {
  def main(args: Array[String]): Unit = {
    val xs = List(1, 2, 3)
    xs.foreach(x => println(x))
  }
}
"#;
        let (_t, st, diags) = crate::typecheck_str_opts(src, &library_opts(&jar));
        assert!(!crate::has_errors(&diags));
        let supplied = st
            .get(st.list_sym)
            .members
            .iter()
            .filter(|&&m| st.get(m).jvm_name.starts_with('('))
            .count();
        assert_eq!(supplied, 0, "read a pickle for a member the prelude has");
    }

    #[test]
    fn pickled_higher_kinded_class_parameter_keeps_its_upper_bound() {
        let Some(jar) = jar() else {
            eprintln!("skip: scala-library jar not present");
            return;
        };
        let src = r#"
object Main {
  def concat(xs: scala.collection.IterableFactoryDefaults[Int, List], ys: List[Int]) = xs ++ ys
}
"#;
        let (_tree, st, diags) = crate::typecheck_str_opts(src, &library_opts(&jar));
        assert!(
            !crate::has_errors(&diags),
            "type errors: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        let cls = crate::classpath::find_by_jvm(&st, "scala/collection/IterableFactoryDefaults")
            .expect("IterableFactoryDefaults should be loaded");
        let cc = st.get(cls).tparams[1];
        assert_eq!(st.get(cc).tparams.len(), 1);
        assert!(
            st.get(cc).bound_hi.is_some(),
            "the bound on IterableFactoryDefaults.CC was erased"
        );
    }

    #[test]
    fn descriptor_arity() {
        assert_eq!(desc_arity("()V"), Some(0));
        assert_eq!(
            desc_arity("(Ljava/lang/String;)Ljava/lang/String;"),
            Some(1)
        );
        assert_eq!(
            desc_arity("(Ljava/lang/Object;Lscala/Function2;)Ljava/lang/Object;"),
            Some(2)
        );
        assert_eq!(desc_arity("(I[[Ljava/lang/String;J)V"), Some(3));
        assert_eq!(desc_arity("no"), None);
    }

    #[test]
    fn singleton_type_names_restore_nested_source_spelling() {
        assert_eq!(
            singleton_type_names("slick.ast.ColumnOption.ColumnOption$PrimaryKey"),
            vec![
                "slick.ast.ColumnOption.ColumnOption$PrimaryKey".to_string(),
                "slick.ast.ColumnOption.PrimaryKey".to_string(),
            ]
        );
        assert_eq!(
            singleton_type_names(
                "slick.relational.RelationalProfile.ColumnOption.RelationalProfile$ColumnOption$Length"
            ),
            vec![
                "slick.relational.RelationalProfile.ColumnOption.RelationalProfile$ColumnOption$Length"
                    .to_string(),
                "slick.relational.RelationalProfile.ColumnOption.Length".to_string(),
            ]
        );
        assert_eq!(
            singleton_type_names("scala.collection.immutable.List$Nil"),
            vec!["scala.collection.immutable.List$Nil".to_string()]
        );
    }
}
