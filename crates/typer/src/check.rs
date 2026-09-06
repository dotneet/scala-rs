#![allow(dead_code)]
//! Namer + typer. Trees are mutated in place (`ty`, `sym`).
//!
//! This file holds the state (`Typer`), the entry points (`typecheck_units`
//! and friends) and the free helper functions. `Typer`'s methods live in the
//! `check_*.rs` files beside it, one `impl Typer` block each, and each of
//! those opens with a few lines saying what it is responsible for:
//!
//! | file | phase |
//! |---|---|
//! | [`crate::check_namer`]    | entering symbols, headers, parents |
//! | [`crate::check_template`] | typing a template, and the rules it must satisfy |
//! | [`crate::check_member`]   | member signatures and bodies, constructors |
//! | [`crate::check_name`]     | statements, imports, unqualified name resolution |
//! | [`crate::check_expr`]     | expression entry and dispatch, `reify` |
//! | [`crate::check_infer`]    | undetermined type parameters, inference, `adapt` |
//! | [`crate::check_select`]   | member selection, access, selection rewrites |
//! | [`crate::check_apply`]    | application typing, receiver-shaped rebuilds |
//! | [`crate::check_args`]     | named arguments, defaults, implicit arguments, tags |
//! | [`crate::check_overload`] | overload resolution and applicability scoring |
//! | [`crate::check_pattern`]  | `match`, patterns, extractors, exhaustiveness |
//! | [`crate::check_types`]    | tree-to-type, projections, classpath supply |

use crate::javaclass::BinaryIndex;
use crate::lazysig::PendingSig;
use crate::prelude::install_prelude;
use crate::symbol::{SymKind, SymbolTable};
use scala_rs_parser::ast::*;
use scala_rs_span::{Diagnostic, Span};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

pub struct TypecheckOptions {
    pub fatal_warnings: bool,
    /// Type Option/List `withFilter` as the scala-library 2.13 shape, StringOps
    /// via `augmentString`, and Iterator. The backend still needs `library_abi`.
    pub library_abi: bool,
    /// Classes loaded from `-cp` (previous compilation's classfiles).
    pub classpath: Vec<ClasspathClass>,
    /// Directories and jars/jmods searched for Java `.class` files (plus the JDK).
    pub binary_path: Vec<PathBuf>,
    /// `-language:postfixOps` / `-language:implicitConversions` / `-language:dynamics`.
    pub language_features: Vec<String>,
    /// `-Xsource-features:<features>`, already reconciled with `-Xsource`
    /// (nsc ignores the whole setting below `-Xsource:3`, so the driver hands
    /// an empty set down in that case).
    pub source_features: crate::source_features::SourceFeatures,
    /// The compiler's own command line, as a macro implementation sees it
    /// through `c.compilerSettings`. nsc rebuilds this from the settings that
    /// were set (`-classpath`, `-d`, `-Xasync`, …); it is how a macro such as
    /// `scala.async.Async.asyncImpl` decides whether `-Xasync` was given.
    pub compiler_settings: Vec<String>,
}

pub(crate) enum OverloadPick {
    Found(SymbolId, Vec<Type>, Type),
    Ambiguous,
    None,
}

/// nsc's `Flags.AccessFlags`, which is what `case-apply-copy-access` copies
/// from the primary constructor onto the synthesized `copy`.
const ACCESS_FLAGS: Flags = Flags(Flags::PRIVATE.0 | Flags::PROTECTED.0 | Flags::LOCAL.0);

/// The access modifier written on a class's primary constructor
/// (`case class C private (x: Int)`), and the two rules
/// `-Xsource-features:case-apply-copy-access` derives from it.
///
/// The rules are *not* the same for `apply` and `copy`, which is easy to get
/// wrong: `case class D protected (x: Int)` compiled by scalac 2.13.16 with
/// the feature has a **protected** `copy` and a **public** `apply`. nsc's
/// `Unapplies.applyAccess` only reacts to `private` and `private[p]`
/// (`mods.hasFlag(PRIVATE) || (!mods.hasFlag(PROTECTED) && mods.hasAccessBoundary)`),
/// and then copies only the `PRIVATE` bit, while `caseClassCopyMeth` copies
/// `flags & AccessFlags` outright.
#[derive(Clone, Debug, Default)]
pub(crate) struct CtorAccess {
    flags: Flags,
    pub(crate) private_within: Option<String>,
}

impl CtorAccess {
    pub(crate) fn of(mods: &Modifiers) -> CtorAccess {
        CtorAccess {
            flags: mods.flags,
            private_within: mods.private_within.clone(),
        }
    }

    /// nsc `Unapplies.applyAccess`: `private` and `private[p]` reach `apply`;
    /// `protected` and `protected[p]` do not.
    pub(crate) fn apply_inherits(&self) -> bool {
        self.flags.contains(Flags::PRIVATE)
            || (!self.flags.contains(Flags::PROTECTED) && self.private_within.is_some())
    }

    /// `caseMods | (inheritedMods.flags & PRIVATE)`.
    pub(crate) fn apply_flags(&self) -> Flags {
        Flags(self.flags.0 & Flags::PRIVATE.0)
    }

    /// `Modifiers(SYNTHETIC | (inheritedMods.flags & AccessFlags), …)`.
    pub(crate) fn copy_flags(&self) -> Flags {
        Flags(self.flags.0 & ACCESS_FLAGS.0)
    }
}

/// How a case class's `copy` was written: `p.copy(…)` or, inside the class
/// itself, a bare `copy(…)` on `this`.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum CopyCallee {
    Qualified,
    Bare,
}

pub(crate) enum CtorDelegation {
    This,
    Super,
    AfterStats,
    Missing,
}

/// A method recovered from a classfile (JVM descriptor).
#[derive(Clone, Debug)]
pub struct ClasspathMethod {
    pub name: String,
    pub desc: String,
}

/// A type recovered from a pickle: the head name plus its type arguments.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ClasspathType {
    pub name: String,
    pub args: Vec<ClasspathType>,
}

impl ClasspathType {
    pub fn simple(name: impl Into<String>) -> Self {
        ClasspathType {
            name: name.into(),
            args: Vec::new(),
        }
    }
}

impl<S: Into<String>> From<S> for ClasspathType {
    fn from(name: S) -> Self {
        ClasspathType::simple(name)
    }
}

/// A type parameter recovered from a pickle, with its own parameters so a type
/// constructor (`F[_]`) does not arrive looking like a proper type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClasspathTypeParam {
    pub name: String,
    pub tparams: Vec<ClasspathTypeParam>,
}

impl ClasspathTypeParam {
    pub fn simple(name: impl Into<String>) -> Self {
        ClasspathTypeParam {
            name: name.into(),
            tparams: Vec::new(),
        }
    }
}

/// A method recovered from our ScalaSignature pickle subset.
#[derive(Clone, Debug)]
pub struct ClasspathPickleMethod {
    pub name: String,
    pub param_names: Vec<String>,
    pub param_types: Vec<ClasspathType>,
    /// Raw pickle flags for each value parameter, including DEFAULTPARAM.
    pub param_flags: Vec<u64>,
    pub ret: ClasspathType,
    pub tparams: Vec<ClasspathTypeParam>,
    pub is_val: bool,
    pub is_ctor: bool,
    pub is_implicit: bool,
    /// The pickle's `DEFERRED`: a declaration, not a definition. Nothing in
    /// the class file says so for a trait -- every member of an interface bar
    /// its `default` methods is `ACC_ABSTRACT` -- and taking them all for
    /// definitions asked for an `override` modifier scalac does not.
    pub is_deferred: bool,
    /// A `var`'s getter (an accessor the pickle does not mark `STABLE`).
    pub is_mutable: bool,
}

/// Binary class/object visible to namer/typer via `-cp`.
#[derive(Clone, Debug)]
pub struct ClasspathClass {
    pub jvm_name: String,
    pub is_module: bool,
    pub methods: Vec<ClasspathMethod>,
    pub pickle: Option<Vec<ClasspathPickleMethod>>,
    /// Class type parameters recovered from the pickle, in order.
    pub pickle_tparams: Vec<ClasspathTypeParam>,
    /// The classfile is an interface -- a Scala trait, or a Java interface.
    pub is_interface: bool,
    /// `super_class` and `interfaces` as JVM internal names. The pickle subset
    /// records member types by *simple* name only, so the classfile header is
    /// the only place the inheritance graph survives intact.
    pub super_name: Option<String>,
    pub interfaces: Vec<String>,
    /// The pickle names `scala.AnyVal` among the parents. The class file
    /// cannot say so -- a value class's superclass is `java/lang/Object` --
    /// and `SymbolTable::is_value_class`, which the whole of erasure and the
    /// `$extension` call path hang off, asks for exactly that parent.
    pub extends_anyval: bool,
}

impl Default for TypecheckOptions {
    fn default() -> Self {
        TypecheckOptions {
            fatal_warnings: false,
            library_abi: false,
            classpath: Vec::new(),
            binary_path: Vec::new(),
            language_features: Vec::new(),
            source_features: crate::source_features::SourceFeatures::default(),
            compiler_settings: Vec::new(),
        }
    }
}

pub struct Typer {
    pub st: SymbolTable,
    pub diags: Vec<Diagnostic>,
    /// Import prefixes a pass could not resolve, as `(file, lo, hi)`.
    /// See `retract_import_prefix_errors`.
    pub(crate) import_prefix_failed: HashSet<(usize, u32, u32)>,
    /// An `import` in the template being typed named a prefix this pass could
    /// not resolve. See `Typer::sig_rerun_safe`.
    pub(crate) import_prefix_missed: bool,
    /// Files holding an `import p._` whose members this compiler cannot
    /// enumerate: the prefix did not resolve at all, or it is a *value* whose
    /// type comes out of a jar, where members are read one name at a time and
    /// a wildcard asks for no name in particular.
    ///
    /// In such a file an unresolved type name is not evidence of anything --
    /// gitbucket writes `import gitbucket.core.model.Profile.profile.blockingApi._`
    /// and then `def f(implicit s: Session)`, and `Session` is a type member
    /// reached through that path. See [`Typer::with_strict_sig_names`].
    pub(crate) opaque_import_files: HashSet<usize>,
    /// A member's signature was taken back to be built again. See
    /// `Typer::leave_sig_for_body_pass`.
    pub(crate) sig_deferred: bool,
    /// The signature pass is on its second and last round, so a signature that
    /// still does not resolve stays as it is and reports.
    pub(crate) sig_final_round: bool,
    pub(crate) file_index: usize,
    /// The text of each unit, by `file_index`.
    ///
    /// A span is only a pair of offsets, and two forms the parser folds into
    /// one node are told apart by the text under them -- `A => B` against
    /// `Function1[A, B]`, `(a, b)` against `Tuple2(a, b)`, `a :: b` against
    /// `b.::(a)`. `crate::reify::Reifier` reads exactly those, and a
    /// `reify { … }` body is *file* text rather than a string the quasiquote
    /// machinery rebuilt, so the file has to reach the typer for the same
    /// readings to work. Empty is allowed: `Reifier::text` then answers `""`
    /// and every reading falls to its written-out branch.
    ///
    /// `Rc` because the reifier borrows one for the length of a build while
    /// the typer is `&mut self` throughout.
    pub(crate) sources: Vec<std::rc::Rc<str>>,
    /// Counter for synthetic names.
    pub(crate) gensym: u32,
    /// Where the argument in each parameter slot was written, as the last call
    /// to `named_arg_slots` left it. `place_named_args` reads it immediately
    /// afterwards and turns it into the entry `SymbolTable::named_arg_order`
    /// carries for the application.
    pub(crate) slot_source: Vec<Option<usize>>,
    /// The order `place_named_args` last produced, waiting for the application
    /// it belongs to to record it under its own node id. Taken (not read) at
    /// the call site, so a nested application cannot inherit it.
    pub(crate) last_named_order: Option<Vec<Option<usize>>>,
    /// Per-binary-name index for *local* classes/objects (`Main$Same$1`,
    /// `Main$Same$2`), keyed by the un-indexed binary name.
    pub(crate) local_class_n: std::collections::HashMap<String, u32>,
    /// Enclosing package clauses; a nested one is relative to the last.
    pub(crate) pkg_nest: Vec<SymbolId>,
    /// Packages a file's `package` clauses actually *open*, by file index.
    ///
    /// SLS 9.2: a clause opens the package it names, not the ones on the way
    /// there. `package p.q` opens only `p.q`, while `package p { package q
    /// { … } }` opens both -- and nsc really does tell the two apart
    /// (2.13.16, with and without `-Xsource:3`). See `expose_unqualified`.
    pub(crate) open_pkgs: HashMap<usize, Vec<SymbolId>>,
    /// Signature pass: fill member types across the whole run before any body
    /// is typed, so a unit can call into one that comes later.
    pub(crate) sigs_only: bool,
    /// The header pass is running: parents and imports only, before any
    /// member has a signature. Anything it works out is provisional, the same
    /// way the signature pass's is. See `Typer::complete_lazy_sig`.
    pub(crate) header_pass: bool,
    /// While set, an unqualified name in *type* position that resolves to
    /// nothing is reported as `not found: type X` instead of being left as the
    /// `Type::Named` placeholder.
    ///
    /// The placeholder is not only a failure: `classpath.rs` builds one for a
    /// jar member whose type the pickle names in a package that has not been
    /// read yet, and a great deal of the run tolerates it on purpose. So the
    /// check is switched on only where nsc is known to have finished
    /// resolution and where silence is an outright acceptance of a program
    /// scalac rejects: the parents of a template, its self type, and the class
    /// a `new` builds. `tree_to_type` recurses, so `extends Seq[Missing]`
    /// points at `Missing`, exactly as nsc does.
    pub(crate) strict_type_names: bool,
    /// The names quantified by the `forSome` clauses currently being resolved,
    /// as a stack (existentials nest). They deliberately stay `Type::Named`
    /// placeholders until `subst_quantified` binds them, so
    /// [`Self::reject_unresolved_type`] must not mistake one for a name that
    /// resolves to nothing.
    pub(crate) exist_quantified: Vec<String>,
    /// Inside a *type pattern* (`case o: TC[_]`), where nsc lets a wildcard
    /// type argument stand for a type constructor. In an ordinary type
    /// position the same `TC[_]` is an existential over a proper type and nsc
    /// rejects it (`_$1 takes no type parameters, expected: 1`).
    pub(crate) pattern_tpt: bool,
    /// Typing the *function* of a constructor pattern (`case x :@ y`), where
    /// nsc's name lookup skips a non-stable method of that name.
    pub(crate) ctor_pattern_fun: bool,
    /// Members whose signature the signature pass already built. Signature
    /// work is not idempotent -- it synthesizes evidence parameters and
    /// default getters -- so the body pass must not redo it.
    pub(crate) sig_done: std::collections::HashSet<(usize, scala_rs_parser::NodeId)>,
    /// Local `lazy val`s whose signature a block already built so that the
    /// statements before them could name them (nsc allows a forward reference
    /// to a `lazy val`, but not to an eager one). Their bodies still wait
    /// their turn, and their signature must not be built a second time.
    pub(crate) lazy_val_presig: std::collections::HashSet<(usize, scala_rs_parser::NodeId)>,
    /// `def`s that stand as a statement of a *block* rather than as a member
    /// of a template. The symbol table cannot say which is which: a local def
    /// in a `val`'s right-hand side is owned by the enclosing *class*, exactly
    /// like a real member, because there is no accessor symbol to own it. Only
    /// `@tailrec` eligibility needs the distinction so far -- a local def is
    /// not a member of anything and so cannot be overridden.
    pub(crate) block_local_defs: std::collections::HashSet<(usize, scala_rs_parser::NodeId)>,
    /// Parent constructor calls whose omitted (implicit / defaulted) argument
    /// list has already been synthesized. `extends P` is walked by the header
    /// pass, the signature pass and the body pass; filling it more than once
    /// would append the arguments twice, and re-running the implicit search on
    /// a later pass would look up an evidence parameter that is no longer in
    /// scope. Keyed by span as well as node id because a parent tree the
    /// compiler itself built (an anonymous class's) carries `NodeId(0)`.
    pub(crate) parent_fill_done:
        std::collections::HashSet<(usize, scala_rs_parser::NodeId, Span, SymbolId)>,
    /// Classes whose companion has already been searched for pickled
    /// implicits. [`Typer::warm_implicit_scope`] reads a class file and asks
    /// the pickle for every implicit name the companion declares; doing that
    /// again for the same class can only find what is already installed.
    pub(crate) warmed_scopes: std::collections::HashSet<u32>,
    /// Classes an argument's type has already been completed from, so that
    /// [`Typer::complete_arg_classes`] costs one walk each and not one per
    /// call. See that function for what the walk is for.
    pub(crate) completed_arg_classes: std::collections::HashSet<u32>,
    /// Set while `type_apply` types the `new C` of a `new C(…)`: that shape
    /// already has an argument list, so the `New` arm must not add one.
    pub(crate) new_is_applied: bool,
    /// Set while a call's arguments are typed with *no* expected type, before
    /// the alternative is picked. An argument that is still a method with an
    /// all-implicit clause must not resolve that clause here: nothing has said
    /// yet what the callee's type parameters are, and `take(empty)` would pick
    /// the one witness in scope instead of the one the parameter asks for.
    pub(crate) typing_call_args: bool,
    /// Set just before the *callee* of an `Apply` / `TypeApply` is typed, and
    /// taken by the first [`Typer::type_expr`] that sees it. A macro
    /// application is expanded at its outermost node the way nsc's is, so
    /// `M.f` must not expand while it stands as the head of `M.f(1)`.
    /// Everything typed *inside* the callee sees the flag already cleared, so
    /// a macro application nested in a receiver (`M.g(1).h`) still expands.
    pub(crate) typing_callee: bool,
    /// How many explicit arguments the `Apply` whose callee is being typed
    /// carries, when that is known. Only [`Typer::search_extension`] reads it,
    /// and only to break a tie: nsc's `adaptToArguments` looks for a view
    /// whose result has a member *applicable to these arguments*, so two
    /// conversions that both offer the name are not ambiguous when only one of
    /// them can be called. gitbucket's `implicit class RichColumn(c1:
    /// Rep[Boolean]) { def &&(c2: => Rep[Boolean], guard: => Boolean) }` ties
    /// with slick's `booleanColumnExtensionMethods` on every `a && b`, and the
    /// tie was reported as `value && is not a member of Rep[Boolean]`.
    /// Cleared while a *qualifier* is typed: the count belongs to the
    /// selection, not to what it is selected from.
    pub(crate) callee_arity: Option<usize>,
    /// The JVM half of the def-macro expander (`crates/typer/src/expand.rs`),
    /// started on the first expansion and killed when the typer is dropped.
    pub(crate) macro_engine: Option<crate::expand::MacroEngine>,
    /// Why the engine could not be started, once it has failed once.
    pub(crate) macro_engine_error: Option<String>,
    /// What `java` is given as its classpath: the run's own binary path, so
    /// the macro implementation, scala-library.jar and scala-reflect.jar are
    /// all on it.
    pub(crate) macro_classpath: Vec<PathBuf>,
    /// Why a macro application could not be expanded, by span. Reported by
    /// `report_macro_calls`, which is the one place that guarantees every
    /// unexpanded call site is an error.
    pub(crate) macro_failures: HashMap<(usize, u32, u32), String>,
    /// How deep the current chain of expansions is.
    pub(crate) macro_depth: u32,
    /// Types handed to the engine as *placeholder* symbols, by the full name
    /// the placeholder carries. A class this run is compiling has no class
    /// file for the engine's mirror to find, so it travels as its name alone
    /// (`crates/typer/src/expand.rs`, `synthType`); this is how the type is
    /// recognised again in the tree that comes back.
    pub(crate) macro_local_tags: HashMap<String, scala_rs_parser::Type>,
    /// Set once any `def f = macro Impl.m` in this run has been resolved.
    /// `type_expr` asks about macro expansion on *every* expression, and
    /// walking an application's spine to find out is not free; almost no
    /// compilation defines a macro at all.
    pub(crate) has_macro_defs: bool,
    /// Set while the arguments of a *parent* constructor call are being
    /// searched for. nsc types those in the constructor's own context, where
    /// `this` does not exist yet, so the class's own and inherited members are
    /// not implicit candidates: `class NullJdbcType extends
    /// DriverJdbcType[Null]` must not answer its parent's `ClassTag[Null]`
    /// with the `implicit val classTag` it is about to inherit from it.
    pub(crate) parent_ctor_scope: bool,
    fatal_warnings: bool,
    pub(crate) library_abi: bool,
    /// Nearest enclosing named method; `None` in class/object constructors.
    pub(crate) return_meth: Option<SymbolId>,
    /// `import scala.language.dynamics` / `-language:dynamics`.
    pub(crate) language_dynamics: bool,
    /// `import scala.language.postfixOps` / `-language:postfixOps`.
    pub(crate) language_postfix_ops: bool,
    /// `import scala.language.implicitConversions` / `-language:implicitConversions`.
    pub(crate) language_implicit_conversions: bool,
    /// `-Xsource-features:<features>` (already gated on `-Xsource:3`).
    pub(crate) source_features: crate::source_features::SourceFeatures,
    /// What `c.compilerSettings` reports to a macro implementation.
    pub(crate) compiler_settings: Vec<String>,
    pub(crate) binary: BinaryIndex,
    pub(crate) completed_java: HashSet<String>,
    /// Overload sets whose alternatives do not all belong to one class's
    /// linearization, keyed by the alternative the tree carries as its symbol.
    ///
    /// `resolve_overload` normally recovers the alternatives' symbols by
    /// re-looking the name up on the owner of the tree's symbol, since
    /// `Type::Overload` carries types and no symbols. That is exact as long as
    /// every alternative is reachable from that owner -- which is how an
    /// ordinary `x.f` set is built. It is not true of a set that spans a class
    /// and its companion object (`supply_from_pickle` installs a companion's
    /// members on the module class, which is not a parent of the class), and
    /// there the re-lookup silently *dropped* every alternative from the other
    /// owner. This remembers the real set for exactly those cases.
    pub(crate) overload_groups: HashMap<u32, Vec<SymbolId>>,
    /// The member type each alternative of an overload had *at its receiver*,
    /// keyed by the head alternative.
    ///
    /// `Type::Overload` carries types without symbols, so `resolve_overload`
    /// re-reads the alternatives off their symbols to learn which one it
    /// picked -- and a declaration read that way has lost the receiver's type
    /// arguments. For a member inherited from a *generic* parent that is the
    /// whole signature: `scala.collection.Seq[A]`'s two `apply`s are
    /// `SeqOps.apply(Int): A` and `PartialFunction[Int, A].apply(Int): A`,
    /// which differ only after instantiation. Read raw, the second one is
    /// `apply(A): B` -- applicable to nothing in particular, and neither
    /// alternative is more specific than the other, so `s(0)` came out
    /// ambiguous. This keeps the instantiated types the selection already
    /// computed.
    pub(crate) overload_member_types: HashMap<u32, Vec<(SymbolId, Type)>>,
    /// nsc's undetermined type variables (`Context.undetparams`).
    ///
    /// An argument is typed with no expected type so that overload resolution
    /// has something to select on, which leaves a polymorphic reference such
    /// as `Map.empty` carrying its own type parameters: `Map[K, V]`. Those
    /// parameters are not fixed -- they are variables the call still has to
    /// solve -- so while this call's alternatives are being weighed they stand
    /// for "anything a parameter can make them" (`undet_compatible`), and they
    /// are solved from the picked parameter type afterwards
    /// (`instantiate_undet_arg`).
    ///
    /// Saved and restored around each application, since typing an argument
    /// runs another application inside this one.
    pub(crate) undet_tvars: Vec<SymbolId>,
    /// Set while two alternatives are being compared for specificity.
    ///
    /// Specificity asks a hypothetical question -- "would `b` accept `a`'s
    /// parameter types as arguments?" -- and the answer must not depend on
    /// what the *actual* call left undetermined. `Set() ++ o` is the case:
    /// the receiver's `?A` is undetermined, so the monomorphic
    /// `++(IterableOnce[?A])` accepts the polymorphic `++[B](IterableOnce[B])`'s
    /// parameter (solve `?A := B`) as readily as the other way round, and the
    /// pair came out `ambiguous overload` where nsc takes the monomorphic one.
    pub(crate) spec_probe: std::cell::Cell<bool>,
    /// Set while an argument list is being retried packed into a tuple.
    ///
    /// The retry builds a fresh `TupleN(a, b)` node and types it as the sole
    /// argument. That node is an application of two arguments in its own
    /// right, so without this flag a `TupleN` that does not typecheck sends
    /// the typer straight back here to wrap it again, forever.
    pub(crate) tupling: bool,
    /// While a template's parent list is being typed: `(the class, the class
    /// that encloses it)`. nsc types parents in the *outer* context, so
    /// `class B extends super.B` inside `trait Mid` means `Mid`'s `super`.
    pub(crate) parent_ctx: Option<(SymbolId, SymbolId)>,
    /// Fills library members the hand-written prelude does not declare, from
    /// their `ScalaSignature` pickles. Only consulted when resolution failed.
    pub(crate) pickle: crate::pickle_supply::PickleSupply,
    /// `import <a value>._`: the class the members came from, and the typed
    /// prefix tree to select them through.
    ///
    /// An object or package prefix needs nothing -- its members are reached
    /// from `MODULE$` or are static -- but `import u._` where `u` is a *value*
    /// leaves an unqualified `Literal` that only means `u.Literal`. Without
    /// the prefix the backend loaded `this`, which is a `ClassCastException`
    /// at run time. This is what `import c.universe._` needs.
    pub(crate) term_import_prefixes: Vec<(SymbolId, Tree)>,
    /// Packages whose jar package object's pickled `type` aliases have been
    /// installed (see `install_pickled_package_aliases`). One read per package.
    pub(crate) pkg_aliases_done: HashSet<u32>,
    /// `(package, package-object module class)` pairs `namer_module` folded
    /// at namer time, as `(pkg, cls)`. The eager fold there only ever sees
    /// `cls`'s *own* members: `rough_parents`, run within the same namer
    /// call, cannot yet resolve a parent declared in a file namer has not
    /// reached, so an inherited member -- `package object data extends
    /// ScalaVersionSpecificPackage` exporting `NonEmptyLazyList`, a `type`
    /// declared on the parent, not in the package object's own body -- is
    /// missing from the eager fold. `typecheck_units_src` redoes this list
    /// once the header pass has resolved every unit's parents for real.
    pub(crate) pending_pkg_folds: Vec<(SymbolId, SymbolId)>,
    /// Pickled package-object aliases whose right-hand side could not be
    /// rebuilt, by simple name: the name then reports *why* it is missing
    /// instead of the bare "not found".
    pub(crate) pkg_alias_gaps: HashMap<String, String>,
    /// Members without a type annotation, keyed by symbol: nsc's lazy
    /// completers (see `crate::lazysig`).
    pub(crate) pending_sigs: HashMap<SymbolId, PendingSig>,
    /// Parameter symbols read back from the library pickle for a hand-written
    /// prelude method, which has none of its own; see
    /// [`crate::prelude_paramnames`]. Kept here rather than on the method so
    /// that only the named-argument path sees them. An empty vector is a
    /// memoised miss.
    pub(crate) prelude_params: HashMap<u32, Vec<Vec<SymbolId>>>,
    /// Signatures being completed right now (nsc's `LOCKED` flag).
    pub(crate) lazy_completing: Vec<SymbolId>,
    /// Symbols a `recursive ... needs type` was already reported for.
    pub(crate) lazy_cyclic: HashSet<SymbolId>,
    /// Definitions completed on demand, waiting to be spliced back.
    pub(crate) lazy_done: HashMap<SymbolId, Tree>,
    /// Definitions already spliced back; both template passes skip them.
    pub(crate) lazy_body_done: HashSet<SymbolId>,
    /// Number of scopes the prelude occupies; they stay in place while a
    /// signature is completed in the scope of its own definition.
    pub(crate) lazy_base_scopes: usize,
    /// Default-argument expressions waiting to be typed. While signatures are
    /// being built the units that come later have not been walked yet, so a
    /// default that names one of their members would see `<notype>`; nsc types
    /// a `name$default$n` body with the other bodies, and so do we.
    pub(crate) defer_default_rhs: bool,
    pub(crate) pending_defaults: Vec<crate::lazysig::PendingDefault>,
    /// The same, for a primary constructor's defaults, whose getters sit on
    /// the companion module (`crate::ctor_defaults`).
    pub(crate) pending_ctor_defaults: Vec<crate::lazysig::PendingCtorDefault>,
    /// Where each default's right-hand side was written. A default with no
    /// `name$default$n` getter to call is spliced into the argument list as
    /// the stored tree; this is the scope it has to be typed in, which is not
    /// the one the call sits in. See `crate::lazysig::DefaultScope`.
    pub(crate) default_scopes: HashMap<SymbolId, crate::lazysig::DefaultScope>,
    /// nsc's `openImplicits`: the (implicit symbol, target type) pairs whose
    /// own implicit parameters are being resolved right now. Used to cut off
    /// diverging expansions (`crate::implicits`).
    pub(crate) open_implicits: std::cell::RefCell<Vec<(SymbolId, Type)>>,
    /// The first expansion cut off as diverging during the current top-level
    /// implicit search, for the diagnostic.
    pub(crate) diverged_implicit: std::cell::RefCell<Option<(SymbolId, Type)>>,
    /// Results of the implicit searches the outermost one in flight has
    /// already answered (`crate::implicits::ImplicitMemo`). Empty whenever no
    /// search is running.
    pub(crate) implicit_memo: std::cell::RefCell<crate::implicits::ImplicitMemo>,
    /// The companion object an implicit was reached *through*, for the ones a
    /// companion only inherits (`object Shape extends RepShapeImplicits`).
    /// Emitting a bare name for those loads `this` and casts it to the trait
    /// that declares them; the receiver is the object. Filled by the companion
    /// half of the implicit scope, read when the reference is materialised.
    pub(crate) implicit_via_module: std::cell::RefCell<HashMap<u32, SymbolId>>,
    /// Modules (or module classes) through which a still-abstract
    /// `Type::TypeMember` was ever selected as a qualified `p.T` (keyed by
    /// `T`'s own defining symbol), for the implicit search's
    /// `collect_type_parts` to add as extra parts.
    ///
    /// `Type::TypeMember` carries only the defining symbol, never the prefix
    /// a source selection actually went through, and dealiasing an
    /// applied-alias use (`NonEmptySet[A]` -> `NonEmptySetImpl.Type[A]`)
    /// throws the prefix away entirely, collapsing to the same shared
    /// abstract member every subclass of `Newtype` inherits without
    /// overriding. Recording the prefix here instead of on the `Type` itself
    /// (a `Type::Refined` "as-seen-from view", the way
    /// `Checker::projected_class_type` records a `Type::Class` prefix) is
    /// deliberate: that view is exact-equality-visible everywhere a bare
    /// `Type::TypeMember` used to compare equal to itself (generic method
    /// type-argument inference in particular, which does not consult
    /// `SymbolTable::as_seen_from_view` the way `is_sub_type` and
    /// `display_type` do), and introduced a real regression --
    /// `WidgetImpl.unwrap(value)` inferring `A` from a wrapped `value` no
    /// longer unified against `unwrap`'s bare `Type[A]` parameter. A side
    /// table only touched by implicit search cannot cause that.
    pub(crate) type_member_prefixes: std::cell::RefCell<HashMap<u32, Vec<SymbolId>>>,
    /// What the last `fill_defaults_and_implicits` pinned down by implicit
    /// search alone: `mk(s)` on `def mk[T: TT](s: String): Seq[Int] => Rep[T]`
    /// has no value argument mentioning `T`, so only the witness fixes it, and
    /// the caller has to put that solution into the result type as well.
    pub(crate) implicit_undet_solved: Vec<(SymbolId, Type)>,
}

/// Highest `TupleN` scala-library defines.
pub(crate) const MAX_TUPLE_ARITY: usize = 22;

/// The `N` of a standard-library `TupleN` / `FunctionN` name, when that is
/// what the name actually is.
///
/// A prefix test is not enough: slick's generated `TupleShape[L, M, U, P]`
/// and `TupleShapeImplicits` are classes of their own, and reading them as
/// 4-tuples turns every use into a type mismatch.
pub(crate) fn numbered_arity(name: &str, prefix: &str) -> Option<usize> {
    let rest = name.strip_prefix(prefix)?;
    if rest.is_empty() || !rest.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    rest.parse().ok()
}

/// Arity of a tuple type, whether it is written structurally (`(A, B)`) or as
/// the class `TupleN[A, B]`. `None` for anything that is not a tuple.
pub(crate) fn tuple_arity(st: &SymbolTable, ty: &Type) -> Option<usize> {
    match ty {
        Type::Tuple(ts) => Some(ts.len()),
        Type::Class { sym, args } => {
            numbered_arity(&st.get(*sym).name, "Tuple").filter(|n| *n == args.len())
        }
        _ => None,
    }
}

pub fn typecheck(tree: &mut Tree, file_index: usize) -> (SymbolTable, Vec<Diagnostic>) {
    typecheck_opts(tree, file_index, &TypecheckOptions::default())
}

pub fn typecheck_opts(
    tree: &mut Tree,
    file_index: usize,
    opts: &TypecheckOptions,
) -> (SymbolTable, Vec<Diagnostic>) {
    let mut units = [(tree, file_index)];
    typecheck_units(&mut units, opts)
}

/// [`typecheck_opts`] with the unit's source text; see [`typecheck_units_src`].
pub fn typecheck_opts_src(
    tree: &mut Tree,
    file_index: usize,
    opts: &TypecheckOptions,
    src: &str,
) -> (SymbolTable, Vec<Diagnostic>) {
    let mut units = [(tree, file_index)];
    let mut sources = vec![String::new(); file_index + 1];
    sources[file_index] = src.to_string();
    typecheck_units_src(&mut units, opts, &sources)
}

/// How many times the header pass may sweep the run. Each round can only
/// turn rough (by-name) parents into resolved ones, so it converges; the cap
/// just bounds the work for deeply nested templates.
const MAX_HEADER_ROUNDS: usize = 3;

/// Typecheck a whole run in one symbol table: every unit is named before any
/// is typed, so a class can reference one defined in another file.
pub fn typecheck_units(
    units: &mut [(&mut Tree, usize)],
    opts: &TypecheckOptions,
) -> (SymbolTable, Vec<Diagnostic>) {
    typecheck_units_src(units, opts, &[])
}

/// [`typecheck_units`] with the text of each unit, indexed by `file_index`.
///
/// Only one thing reads it, and it is not optional there: `reify { … }` hands
/// its body to `crate::reify::Reifier`, which tells two forms the parser folds
/// into one node apart by the text under their span (`A => B` against
/// `Function1[A, B]`, `(a, b)` against `Tuple2(a, b)`). A quasiquote's body is
/// a string that module rebuilt and can therefore be handed straight back; a
/// `reify` body is *file* text, so the file has to come with it. A caller with
/// no text -- every unit test that types a snippet -- passes none, and each of
/// those readings falls to its written-out branch.
pub fn typecheck_units_src(
    units: &mut [(&mut Tree, usize)],
    opts: &TypecheckOptions,
    sources: &[String],
) -> (SymbolTable, Vec<Diagnostic>) {
    let first = units.first().map(|(_, i)| *i).unwrap_or(0);
    let mut t = Typer::new(first, opts);
    t.sources = sources
        .iter()
        .map(|s| std::rc::Rc::from(s.as_str()))
        .collect();
    t.fatal_warnings = opts.fatal_warnings;
    crate::classpath::install_classpath(&mut t.st, &opts.classpath);
    t.link_tuple_products();
    t.link_string_parents();
    t.defer_default_rhs = true;
    for (tree, file_index) in units.iter_mut() {
        t.file_index = *file_index;
        t.namer(tree);
        t.register_sealed_from_namer(tree);
    }
    {
        // Class headers before member types, across every unit: a class can
        // inherit from one whose own superclass chain is declared in a file
        // that comes later on the command line, and inherited names have to
        // be visible while that class's members are typed.
        let diag_mark = t.diags.len();
        let saved_lang = (
            t.language_dynamics,
            t.language_postfix_ops,
            t.language_implicit_conversions,
        );
        t.header_pass = true;
        for _ in 0..MAX_HEADER_ROUNDS {
            let mut changed = false;
            for (tree, file_index) in units.iter_mut() {
                t.file_index = *file_index;
                changed |= t.parents_pass(tree, false);
            }
            if !changed {
                break;
            }
        }
        // Once the parents are known, one more sweep types the constructor
        // parameters, so `extends Parent(x)` in any file meets a complete
        // primary constructor.
        for (tree, file_index) in units.iter_mut() {
            t.file_index = *file_index;
            t.parents_pass(tree, true);
        }
        t.header_pass = false;
        // The header pass exists only to resolve parents; it types imports
        // and parent trees before signatures are known, so anything it
        // complains about is reported for real by the passes below.
        t.diags.truncate(diag_mark);
        t.language_dynamics = saved_lang.0;
        t.language_postfix_ops = saved_lang.1;
        t.language_implicit_conversions = saved_lang.2;
    }
    // Redo the package-object member fold `namer_module` did eagerly, now
    // that the header pass above has resolved every unit's parents for
    // real. `members_including_inherited` reaches a member declared on a
    // package object's parent class rather than in its own body -- cats'
    // `package object data extends ScalaVersionSpecificPackage`, which is
    // where `type NonEmptyLazyList` actually lives -- and that parent may
    // be declared in a file namer had not reached yet when the eager fold
    // ran.
    for (pkg, cls) in std::mem::take(&mut t.pending_pkg_folds) {
        let mems = t.st.members_including_inherited(cls);
        for mem in mems {
            if !t.st.get(pkg).members.contains(&mem) {
                t.st.get_mut(pkg).members.push(mem);
            }
        }
    }
    // After the header pass: loading `scala.collection.IterableFactory` pulls
    // in the `scala` package object, and doing that before any source has
    // named a `scala.*` type made `install_pickled_package_aliases` run too
    // early -- `type Integral[T] = scala.math.Integral[T]` came out
    // unresolvable, and the memo kept it that way for the rest of the run.
    t.link_collection_factories();
    {
        // Member types first, across every unit: typing a body may call a
        // member declared further down the file, or in a file that comes
        // later on the command line.
        t.sigs_only = true;
        for (tree, file_index) in units.iter_mut() {
            t.file_index = *file_index;
            t.typer(tree);
        }
        // A member written under an `import` whose prefix only another unit's
        // signature settles cannot be built on the round above, whichever
        // order the units are in: gitbucket's `trait AccountService { import
        // gitbucket.core.model.Profile.profile.blockingApi._; def
        // getAccountByUserName(…)(implicit s: Session) }` needs `Profile`'s
        // own signatures. `leave_sig_for_body_pass` takes such a member back;
        // this round builds it with every unit's signatures in place. It has
        // to happen before *any* body is typed, because a caller's unit may
        // come first on the command line -- gitbucket's `controller/` sorts
        // before `service/` -- and would otherwise read the signature that was
        // taken back.
        if t.sig_deferred {
            t.sig_final_round = true;
            for (tree, file_index) in units.iter_mut() {
                t.file_index = *file_index;
                t.typer(tree);
            }
            t.sig_final_round = false;
        }
        t.sigs_only = false;
    }
    // Default arguments are bodies, not signatures: typing them during the
    // pass above would let one name only the members of the units that come
    // before its own on the command line.
    t.defer_default_rhs = false;
    t.type_pending_defaults();
    for (tree, file_index) in units.iter_mut() {
        t.file_index = *file_index;
        t.typer(tree);
        t.report_macro_calls(tree);
        t.strip_macro_defs(tree);
    }
    // Class headers are typed by both passes, so the same complaint about a
    // parent or self type is raised twice. Member signatures are built once
    // (see `sig_done`), so their diagnostics survive here.
    dedup_diags(&mut t.diags);
    (t.st, t.diags)
}

impl Typer {
    pub fn new(file_index: usize, opts: &TypecheckOptions) -> Self {
        let mut st = SymbolTable::new();
        install_prelude(
            &mut st,
            opts.library_abi,
            crate::prelude_reflect::want_context_stub(&opts.classpath),
        );
        st.prelude_end = st.symbols.len() as u32;
        let lazy_base_scopes = st.scopes.len();
        Typer {
            st,
            diags: Vec::new(),
            import_prefix_failed: HashSet::new(),
            import_prefix_missed: false,
            opaque_import_files: HashSet::new(),
            sig_deferred: false,
            sig_final_round: false,
            file_index,
            sources: Vec::new(),
            gensym: 0,
            slot_source: Vec::new(),
            last_named_order: None,
            local_class_n: std::collections::HashMap::new(),
            pkg_nest: Vec::new(),
            open_pkgs: HashMap::new(),
            sigs_only: false,
            header_pass: false,
            strict_type_names: false,
            exist_quantified: Vec::new(),
            pattern_tpt: false,
            ctor_pattern_fun: false,
            sig_done: std::collections::HashSet::new(),
            lazy_val_presig: std::collections::HashSet::new(),
            block_local_defs: std::collections::HashSet::new(),
            parent_fill_done: std::collections::HashSet::new(),
            warmed_scopes: std::collections::HashSet::new(),
            completed_arg_classes: std::collections::HashSet::new(),
            new_is_applied: false,
            typing_call_args: false,
            typing_callee: false,
            callee_arity: None,
            macro_engine: None,
            macro_engine_error: None,
            macro_classpath: opts.binary_path.clone(),
            macro_failures: HashMap::new(),
            macro_depth: 0,
            macro_local_tags: HashMap::new(),
            has_macro_defs: false,
            parent_ctor_scope: false,
            fatal_warnings: opts.fatal_warnings,
            library_abi: opts.library_abi,
            return_meth: None,
            language_dynamics: language_flag_enabled(&opts.language_features, "dynamics"),
            language_postfix_ops: language_flag_enabled(&opts.language_features, "postfixOps"),
            language_implicit_conversions: language_flag_enabled(
                &opts.language_features,
                "implicitConversions",
            ),
            source_features: opts.source_features,
            compiler_settings: opts.compiler_settings.clone(),
            binary: BinaryIndex::from_user_paths(opts.binary_path.clone()),
            completed_java: HashSet::new(),
            overload_groups: HashMap::new(),
            overload_member_types: HashMap::new(),
            undet_tvars: Vec::new(),
            spec_probe: std::cell::Cell::new(false),
            tupling: false,
            parent_ctx: None,
            pickle: crate::pickle_supply::PickleSupply::new(),
            term_import_prefixes: Vec::new(),
            pkg_aliases_done: HashSet::new(),
            pending_pkg_folds: Vec::new(),
            pkg_alias_gaps: HashMap::new(),
            pending_sigs: HashMap::new(),
            prelude_params: HashMap::new(),
            lazy_completing: Vec::new(),
            lazy_cyclic: HashSet::new(),
            lazy_done: HashMap::new(),
            lazy_body_done: HashSet::new(),
            lazy_base_scopes,
            defer_default_rhs: false,
            pending_ctor_defaults: Vec::new(),
            default_scopes: HashMap::new(),
            pending_defaults: Vec::new(),
            open_implicits: std::cell::RefCell::new(Vec::new()),
            diverged_implicit: std::cell::RefCell::new(None),
            implicit_memo: std::cell::RefCell::new(Default::default()),
            implicit_via_module: std::cell::RefCell::new(HashMap::new()),
            type_member_prefixes: std::cell::RefCell::new(HashMap::new()),
            implicit_undet_solved: Vec::new(),
        }
    }

    /// How many errors have been reported since `mark`, a `self.diags.len()`
    /// taken before a speculative typing.
    pub(crate) fn error_count_since(&self, mark: usize) -> usize {
        self.diags[mark..]
            .iter()
            .filter(|d| d.level == scala_rs_span::Level::Error)
            .count()
    }

    pub(crate) fn error(&mut self, span: Span, msg: impl Into<String>) {
        self.diags
            .push(Diagnostic::error(self.file_index, span, msg));
    }

    pub(crate) fn warning(&mut self, span: Span, msg: impl Into<String>) {
        if self.fatal_warnings {
            self.error(span, msg);
        } else {
            self.diags
                .push(Diagnostic::warning(self.file_index, span, msg));
        }
    }

    pub(crate) fn fresh(&mut self, prefix: &str) -> String {
        self.gensym += 1;
        format!("{prefix}${}$", self.gensym)
    }
}

/// Whether a built signature still contains a name that resolved to nothing.
///
/// `Type::Named` is what `resolve_type_name` hands back when the lookup found
/// no symbol at all, and `Type::Error` is a name that was also *reported*. The
/// two are not interchangeable, and the difference is why a `def` cannot be
/// judged by its diagnostics alone: a bare `Ident` in a parameter position is
/// not under [`Typer::strict_type_names`], so `def f(implicit s: Session)`
/// written under an import that has not resolved yet silently becomes
/// `Named { "Session" }` and says nothing, while `Rep[Boolean]` in the same
/// signature is an `AppliedTypeTree` and does report. gitbucket shows both
/// halves of that, from the very same methods: 36 `not found: type Rep` at the
/// definitions, and 187 `could not find implicit value of type Session` at
/// their callers -- an implicit search for a placeholder cannot succeed.
///
/// `NoType` is deliberately not "unresolved": a `def` with no written result
/// type has one until `lazysig` completes it, and that is the normal state of
/// most method bodies.
pub(crate) fn type_mentions_unresolved(ty: &Type) -> bool {
    match ty {
        Type::Named { .. } | Type::Error => true,
        Type::Array(t) | Type::ByName(t) | Type::Repeated(t) => type_mentions_unresolved(t),
        Type::Tuple(ts) | Type::Overload(ts) => ts.iter().any(type_mentions_unresolved),
        Type::Function { params, ret } => {
            params.iter().any(type_mentions_unresolved) || type_mentions_unresolved(ret)
        }
        Type::Method { paramss, ret } => {
            paramss.iter().flatten().any(type_mentions_unresolved) || type_mentions_unresolved(ret)
        }
        Type::Class { args, .. } => args.iter().any(type_mentions_unresolved),
        Type::Applied { ctor, args } => {
            type_mentions_unresolved(ctor) || args.iter().any(type_mentions_unresolved)
        }
        Type::BoundedWildcard { lo, hi } => {
            lo.as_deref().is_some_and(type_mentions_unresolved)
                || hi.as_deref().is_some_and(type_mentions_unresolved)
        }
        Type::SingleType { prefix, .. } => type_mentions_unresolved(prefix),
        Type::Annotated { tpe, .. } => type_mentions_unresolved(tpe),
        Type::Refined { parents, .. } => parents.iter().any(type_mentions_unresolved),
        _ => false,
    }
}

pub(crate) fn is_tuple2_elem_map(name: &str) -> bool {
    matches!(
        name,
        "Map" | "HashMap" | "LinkedHashMap" | "SortedMap" | "TreeMap" | "MapView"
    )
}

pub(crate) fn is_tailrec_annot(path: &str) -> bool {
    matches!(
        path,
        "tailrec" | "annotation.tailrec" | "scala.annotation.tailrec"
    )
}

pub(crate) fn is_native_annot(path: &str) -> bool {
    matches!(path, "native" | "scala.native")
}

pub(crate) fn is_override_annot(path: &str) -> bool {
    matches!(path, "Override" | "java.lang.Override")
}

/// The `IterableOps` / `SeqOps` / `MapOps` / `SetOps` members whose 2.13
/// signature returns the receiver's own collection (`C`, or `CC[B]` / `CC[K2,
/// V2]` for the ones that widen the element type). The conversions (`toSeq`,
/// `toList`, …) are deliberately absent: those really do return the class they
/// name.
///
/// `-` / `removed` / `incl` / `excl` are here even though they erase to a
/// *named* class (`(Object)Lscala/collection/immutable/Map;` for `MapOps`):
/// `maybe_unbox_erased_result` casts a result whose static type is narrower
/// than the descriptor's, so `TreeMap - key` is a `TreeMap` at both ends.
///
/// `+` and `-` reach this for every receiver, arithmetic included; only a
/// `scala.collection` class rebuilds (`rebuild_from_receiver`), so `1 + 2` and
/// `"a" + b` are untouched.
pub(crate) fn returns_receiver_collection(name: &str) -> bool {
    matches!(
        name,
        "filter"
            | "filterNot"
            | "take"
            | "takeRight"
            | "takeWhile"
            | "drop"
            | "dropRight"
            | "dropWhile"
            | "slice"
            | "tail"
            | "init"
            | "reverse"
            | "distinct"
            | "distinctBy"
            | "sorted"
            | "sortBy"
            | "sortWith"
            | "patch"
            | "updated"
            | "padTo"
            | "diff"
            | "intersect"
            | "appended"
            | "prepended"
            | "appendedAll"
            | "prependedAll"
            | "$plus$plus"
            | "++"
            | "$colon$plus"
            | ":+"
            | "$plus$colon"
            | "+:"
            | "$minus"
            | "-"
            | "$minus$minus"
            | "--"
            | "$plus"
            | "+"
            | "removed"
            | "removedAll"
            | "incl"
            | "excl"
            | "concat"
    )
}

/// `T @annot` read as `T`. An annotation says nothing about what members a
/// type has, so every question about its *shape* has to look through it.
pub(crate) fn strip_annotations(ty: &Type) -> &Type {
    let mut t = ty;
    while let Type::Annotated { tpe, .. } = t {
        t = tpe;
    }
    t
}

pub(crate) fn peel_empty_annot(ty: &Type) -> Type {
    match ty {
        Type::Annotated { tpe, .. } if tpe.is_no_type() => Type::NoType,
        Type::Annotated { tpe, .. } => peel_empty_annot(tpe),
        other => other.clone(),
    }
}

pub(crate) fn fill_empty_annot(ascr: Type, found: &Type) -> Type {
    match ascr {
        Type::Annotated { tpe, annot } if tpe.is_no_type() => Type::Annotated {
            tpe: Box::new(found.clone()),
            annot,
        },
        Type::Annotated { tpe, annot } => Type::Annotated {
            tpe: Box::new(fill_empty_annot(*tpe, found)),
            annot,
        },
        other => other,
    }
}

pub(crate) fn tree_has_switch(t: &Tree) -> bool {
    fn ty_has_switch(ty: &Type) -> bool {
        match ty {
            Type::Annotated { annot, tpe } => {
                annot.rsplit('.').next() == Some("switch") || ty_has_switch(tpe)
            }
            _ => false,
        }
    }
    match &t.kind {
        TreeKind::Typed { tpt, expr } => annot_tree_is_switch(tpt) || tree_has_switch(expr),
        _ => ty_has_switch(&t.ty),
    }
}

fn annot_tree_is_switch(tpt: &Tree) -> bool {
    match &tpt.kind {
        TreeKind::AnnotatedTypeTree { annot, tpt } => {
            let path = annot.annotation_path();
            let simple = path.rsplit('.').next().unwrap_or(path.as_str());
            simple == "switch" || annot_tree_is_switch(tpt)
        }
        _ => false,
    }
}

pub(crate) fn match_can_switch(sel_ty: &Type, cases: &[scala_rs_parser::CaseDef]) -> bool {
    switch_case_keys(sel_ty, cases).is_some()
}

fn switch_case_keys(
    sel_ty: &Type,
    cases: &[scala_rs_parser::CaseDef],
) -> Option<Vec<(i32, usize)>> {
    let core = peel_type_annot(sel_ty);
    if !matches!(core, Type::Int | Type::Char) {
        return None;
    }
    let mut keys = Vec::new();
    let mut default = false;
    for (i, c) in cases.iter().enumerate() {
        if !c.guard.is_empty() {
            return None;
        }
        match switch_pat_key(&c.pat) {
            Some(SwitchPat::Key(k)) => keys.push((k, i)),
            Some(SwitchPat::Default) => {
                if default {
                    return None;
                }
                default = true;
            }
            None => return None,
        }
    }
    if keys.is_empty() {
        return None;
    }
    Some(keys)
}

pub(crate) fn peel_type_annot(ty: &Type) -> &Type {
    match ty {
        Type::Annotated { tpe, .. } => peel_type_annot(tpe),
        t => t,
    }
}

enum SwitchPat {
    Key(i32),
    Default,
}

fn switch_pat_key(pat: &Tree) -> Option<SwitchPat> {
    match &pat.kind {
        TreeKind::Literal { lit: Lit::Int(n) } => Some(SwitchPat::Key(*n)),
        TreeKind::Literal { lit: Lit::Char(c) } => Some(SwitchPat::Key(*c as i32)),
        TreeKind::Wildcard | TreeKind::Empty => Some(SwitchPat::Default),
        TreeKind::Ident { name } => {
            let is_varid = name
                .chars()
                .next()
                .is_some_and(|c| c.is_lowercase() || c == '_');
            if is_varid {
                Some(SwitchPat::Default)
            } else {
                None
            }
        }
        TreeKind::Bind { body, .. } => switch_pat_key(body),
        TreeKind::Typed { expr, .. } => switch_pat_key(expr),
        _ => None,
    }
}

pub(crate) fn annot_first_string(tree: &Tree) -> Option<String> {
    match &tree.kind {
        TreeKind::Apply { args, .. } => args.iter().find_map(annot_first_string),
        TreeKind::Assign { rhs, .. } => annot_first_string(rhs),
        TreeKind::Literal {
            lit: Lit::String(s),
        } => Some(s.clone()),
        _ => None,
    }
}

pub(crate) fn method_value_params(ty: &Type) -> Vec<Type> {
    match ty {
        Type::Method { paramss, .. } => paramss.iter().flatten().cloned().collect(),
        Type::Function { params, .. } => params.clone(),
        _ => Vec::new(),
    }
}

fn is_rec_apply(tree: &Tree, meth: SymbolId) -> bool {
    match &tree.kind {
        TreeKind::Apply { fun, .. } | TreeKind::TypeApply { fun, .. } => {
            rec_fun_is_method(fun, meth)
        }
        _ => false,
    }
}

fn rec_fun_is_method(tree: &Tree, meth: SymbolId) -> bool {
    if meth.is_none() {
        return false;
    }
    match &tree.kind {
        TreeKind::TypeApply { fun, .. } => rec_fun_is_method(fun, meth),
        TreeKind::Ident { .. } => tree.sym == meth,
        TreeKind::Select { .. } => tree.sym == meth,
        _ => tree.sym == meth,
    }
}

// Every argument clause and the receiver is evaluated before the recursive
// call. Looking only at the outer Apply's args missed recursion in a receiver
// (`f(0).f(n - 1)`) or in an earlier curried argument clause.
fn count_tailrec_call_inputs(
    tree: &Tree,
    meth: SymbolId,
    nullary: bool,
    n_tail: &mut u32,
    n_nontail: &mut u32,
) {
    match &tree.kind {
        TreeKind::Apply { fun, args } => {
            count_tailrec_call_inputs(fun, meth, nullary, n_tail, n_nontail);
            for arg in args {
                count_tailrec_calls(arg, meth, nullary, false, n_tail, n_nontail);
            }
        }
        TreeKind::TypeApply { fun, .. } => {
            count_tailrec_call_inputs(fun, meth, nullary, n_tail, n_nontail);
        }
        TreeKind::Select { qual, .. } => {
            count_tailrec_calls(qual, meth, nullary, false, n_tail, n_nontail);
        }
        _ => {}
    }
}

pub(crate) fn count_tailrec_calls(
    tree: &Tree,
    meth: SymbolId,
    nullary: bool,
    tail: bool,
    n_tail: &mut u32,
    n_nontail: &mut u32,
) {
    // A parameterless method is called by naming it: `n.sourceNominalType`
    // is a `Select`, with no `Apply` wrapped round it.
    if nullary
        && tree.sym == meth
        && !meth.is_none()
        && matches!(&tree.kind, TreeKind::Select { .. } | TreeKind::Ident { .. })
    {
        if tail {
            *n_tail += 1;
        } else {
            *n_nontail += 1;
        }
        if let TreeKind::Select { qual, .. } = &tree.kind {
            count_tailrec_calls(qual, meth, nullary, false, n_tail, n_nontail);
        }
        return;
    }
    if is_rec_apply(tree, meth) {
        if tail {
            *n_tail += 1;
        } else {
            *n_nontail += 1;
        }
        count_tailrec_call_inputs(tree, meth, nullary, n_tail, n_nontail);
        return;
    }
    match &tree.kind {
        TreeKind::If { cond, thenp, elsep } => {
            count_tailrec_calls(cond, meth, nullary, false, n_tail, n_nontail);
            count_tailrec_calls(thenp, meth, nullary, tail, n_tail, n_nontail);
            count_tailrec_calls(elsep, meth, nullary, tail, n_tail, n_nontail);
        }
        TreeKind::Block { stats, expr } => {
            for s in stats {
                count_tailrec_calls(s, meth, nullary, false, n_tail, n_nontail);
            }
            count_tailrec_calls(expr, meth, nullary, tail, n_tail, n_nontail);
        }
        TreeKind::Match { selector, cases } => {
            count_tailrec_calls(selector, meth, nullary, false, n_tail, n_nontail);
            for c in cases {
                if !c.guard.is_empty() {
                    count_tailrec_calls(&c.guard, meth, nullary, false, n_tail, n_nontail);
                }
                count_tailrec_calls(&c.body, meth, nullary, tail, n_tail, n_nontail);
            }
        }
        TreeKind::Apply { fun, args } => {
            count_tailrec_calls(fun, meth, nullary, false, n_tail, n_nontail);
            for a in args {
                count_tailrec_calls(a, meth, nullary, false, n_tail, n_nontail);
            }
        }
        TreeKind::TypeApply { fun, args } => {
            count_tailrec_calls(fun, meth, nullary, tail, n_tail, n_nontail);
            let _ = args;
        }
        TreeKind::Select { qual, .. } => {
            count_tailrec_calls(qual, meth, nullary, false, n_tail, n_nontail)
        }
        TreeKind::Typed { expr, .. } => {
            count_tailrec_calls(expr, meth, nullary, tail, n_tail, n_nontail)
        }
        TreeKind::Assign { lhs, rhs } => {
            count_tailrec_calls(lhs, meth, nullary, false, n_tail, n_nontail);
            count_tailrec_calls(rhs, meth, nullary, false, n_tail, n_nontail);
        }
        TreeKind::While { cond, body } | TreeKind::DoWhile { cond, body } => {
            count_tailrec_calls(cond, meth, nullary, false, n_tail, n_nontail);
            count_tailrec_calls(body, meth, nullary, false, n_tail, n_nontail);
        }
        TreeKind::Try {
            block,
            catches,
            finalizer,
        } => {
            count_tailrec_calls(block, meth, nullary, false, n_tail, n_nontail);
            for c in catches {
                count_tailrec_calls(&c.body, meth, nullary, false, n_tail, n_nontail);
            }
            if !finalizer.is_empty() {
                count_tailrec_calls(finalizer, meth, nullary, false, n_tail, n_nontail);
            }
        }
        TreeKind::Function { body, .. } => {
            count_tailrec_calls(body, meth, nullary, false, n_tail, n_nontail);
        }
        TreeKind::Return { expr } | TreeKind::Throw { expr } => {
            count_tailrec_calls(expr, meth, nullary, false, n_tail, n_nontail);
        }
        _ => {}
    }
}

pub(crate) fn f_kind_name(kind: scala_rs_parser::finterp::FConvKind) -> &'static str {
    use scala_rs_parser::finterp::FConvKind;
    match kind {
        FConvKind::Integral => "integral type",
        FConvKind::Floating => "floating type",
        FConvKind::Character => "character/integral type",
        FConvKind::General => "Any",
        FConvKind::Unsupported => "a supported conversion",
    }
}

pub(crate) fn is_inferable_param_pt(pt: &Type) -> bool {
    !matches!(
        pt,
        Type::NoType | Type::Error | Type::Any | Type::AnyRef | Type::AnyVal | Type::Overload(_)
    )
}

/// The parameter and result types of an expected function type, for the
/// `Function1`/`FunctionN` spellings the typer produces.
pub(crate) fn function_sig(pt: &Type) -> Option<(Vec<Type>, Type)> {
    match pt {
        Type::Function { params, ret } => Some((params.clone(), (**ret).clone())),
        Type::Class { sym: _, args } if args.len() >= 2 && is_function_pt(pt) => {
            let (last, init) = args.split_last()?;
            Some((init.to_vec(), last.clone()))
        }
        _ => None,
    }
}

pub(crate) fn is_function_pt(pt: &Type) -> bool {
    match pt {
        Type::Function { .. } => true,
        Type::Named { name, .. }
            if (name.starts_with("Function") && name != "Function")
                || name == "PartialFunction" =>
        {
            true
        }
        _ => false,
    }
}

/// `scala.util.Either` and its two cases. 2.13 made them right-biased, so the
/// "element" of an `Either[A, B]` is its `B`. Matched by JVM name so a user
/// class that happens to be called `Either` keeps the ordinary rule.
pub(crate) fn is_right_biased_either(st: &SymbolTable, id: SymbolId) -> bool {
    if id.is_none() {
        return false;
    }
    matches!(
        st.get(id).jvm_name.as_str(),
        "scala/util/Either" | "scala/util/Left" | "scala/util/Right"
    )
}

/// Index into the flattened parameter list where the clause holding `flat_idx`
/// begins. `synthesize_default_getters` needs it to tell "a parameter of an
/// earlier clause" (which a default may name) from "an earlier parameter of my
/// own clause" (which nsc forbids).
pub(crate) fn clause_start_of(paramss_ids: &[Vec<SymbolId>], flat_idx: usize) -> usize {
    let mut start = 0usize;
    for clause in paramss_ids {
        if flat_idx < start + clause.len() {
            return start;
        }
        start += clause.len();
    }
    start
}

/// Whether an untyped tree mentions any of `names` as a bare identifier. Used
/// only to decide whether a default body reads an earlier parameter of its own
/// clause, so a false positive costs one extra getter parameter, never
/// correctness.
pub(crate) fn tree_names_any(tree: &Tree, names: &[String]) -> bool {
    let mut t = tree.clone();
    fn walk(t: &mut Tree, names: &[String]) -> bool {
        if let TreeKind::Ident { name } = &t.kind {
            if names.iter().any(|n| n == name) {
                return true;
            }
        }
        crate::lazy_local::children_mut(t)
            .into_iter()
            .any(|c| walk(c, names))
    }
    walk(&mut t, names)
}

/// The parser desugars `{ case … }` into `x$pf => x$pf match { case … }`.
pub(crate) fn is_case_block_literal(vparams: &[Tree], body: &Tree) -> bool {
    vparams.len() == 1
        && vparams[0].name() == Some(PF_PARAM)
        && matches!(&body.kind, TreeKind::Match { .. })
}

/// Name the parser gives the synthesized parameter of a `{ case … }` literal.
const PF_PARAM: &str = "x$pf";

pub(crate) fn is_partial_function_sym(st: &SymbolTable, id: SymbolId) -> bool {
    if id.is_none() {
        return false;
    }
    let s = st.get(id);
    s.name == "PartialFunction"
        && (s.jvm_name == "scala/PartialFunction" || s.jvm_name.ends_with("PartialFunction"))
}

pub(crate) fn partial_function_type(st: &SymbolTable, pt: &Type) -> Option<(Type, Type)> {
    match pt {
        Type::Named { name, args } if name == "PartialFunction" && args.len() == 2 => {
            Some((args[0].clone(), args[1].clone()))
        }
        Type::Class { sym, args } if is_partial_function_sym(st, *sym) => {
            if args.len() >= 2 {
                Some((args[0].clone(), args[1].clone()))
            } else {
                Some((Type::Any, Type::Any))
            }
        }
        _ => None,
    }
}

pub(crate) fn unwrap_fn0_or_byname(ty: &Type) -> Type {
    match ty {
        Type::ByName(t) => (**t).clone(),
        Type::Function { params, ret } if params.is_empty() => (**ret).clone(),
        other => other.clone(),
    }
}

/// The scrutinee of a stable-identifier pattern, with everything it does not
/// yet know replaced by `_`.
///
/// `case ScalaBaseType.byteType =>` inside `def f[T](t: ScalaType[T])` compares
/// a `ScalaNumericType[Byte]` with a `ScalaType[T]`. nsc accepts it -- `T`
/// *could* be `Byte`, and the pattern is only an `==` at run time -- so a
/// scrutinee that still names a type parameter or an abstract type member
/// rules nothing out. Only the arguments are relaxed: the head class still has
/// to line up.
pub(crate) fn relax_abstract_targs(ty: &Type) -> Type {
    fn relax(t: &Type) -> Type {
        match t {
            Type::TypeParam(_) | Type::TypeMember(_) => Type::Wildcard,
            _ => relax_abstract_targs(t),
        }
    }
    match ty {
        Type::Class { sym, args } if !args.is_empty() => Type::Class {
            sym: *sym,
            args: args.iter().map(relax).collect(),
        },
        Type::Tuple(ts) if !ts.is_empty() => Type::Tuple(ts.iter().map(relax).collect()),
        Type::Array(t) => Type::Array(Box::new(relax(t))),
        _ => ty.clone(),
    }
}

pub(crate) fn pattern_has_star(pat: &Tree) -> bool {
    match &pat.kind {
        TreeKind::Star { .. } => true,
        TreeKind::Bind { body, .. } => pattern_has_star(body),
        TreeKind::Typed { expr, .. } => pattern_has_star(expr),
        _ => false,
    }
}

pub(crate) fn tree_contains_this(tree: &Tree) -> bool {
    match &tree.kind {
        TreeKind::This { .. } => true,
        TreeKind::Select { qual, .. } | TreeKind::Typed { expr: qual, .. } => {
            tree_contains_this(qual)
        }
        TreeKind::Apply { fun, args } | TreeKind::TypeApply { fun, args } => {
            tree_contains_this(fun) || args.iter().any(tree_contains_this)
        }
        TreeKind::Block { stats, expr } => {
            stats.iter().any(tree_contains_this) || tree_contains_this(expr)
        }
        TreeKind::If { cond, thenp, elsep } => {
            tree_contains_this(cond) || tree_contains_this(thenp) || tree_contains_this(elsep)
        }
        TreeKind::Assign { lhs, rhs } => tree_contains_this(lhs) || tree_contains_this(rhs),
        TreeKind::New { tpt } => tree_contains_this(tpt),
        _ => false,
    }
}

/// Overload applicability: `Tuple2[Any, Any]` matches `Tuple2[K, V]` when `K`/`V`
/// are type parameters. Not used for implicit search (`is_sub_type`).
pub(crate) fn class_ctor_matches_typeparam_args(arg: &Type, param: &Type) -> bool {
    match (arg, param) {
        (Type::Class { sym: sa, args: aa }, Type::Class { sym: sp, args: pa })
            if sa == sp && aa.len() == pa.len() =>
        {
            aa.iter().zip(pa.iter()).all(|(a, p)| {
                matches!(p, Type::TypeParam(_)) || a == p || class_ctor_matches_typeparam_args(a, p)
            })
        }
        (Type::Tuple(aa), Type::Class { args: pa, .. }) if aa.len() == pa.len() => {
            aa.iter().zip(pa.iter()).all(|(a, p)| {
                matches!(p, Type::TypeParam(_)) || a == p || class_ctor_matches_typeparam_args(a, p)
            })
        }
        (Type::Class { args: aa, .. }, Type::Tuple(pa)) if aa.len() == pa.len() => {
            aa.iter().zip(pa.iter()).all(|(a, p)| {
                matches!(p, Type::TypeParam(_)) || a == p || class_ctor_matches_typeparam_args(a, p)
            })
        }
        // `def f[A](t: X[A] with Y[A])`: a *compound* parameter that mentions
        // the alternative's own type parameter is matched component by
        // component, exactly like the class case above. slick spells its
        // `BaseColumnType[T] = ScalaType[T] with BaseTypedType[T]` that way and
        // passes an `implicitly[BaseColumnType[U]]` to it, which scored no
        // match at all -- `is_sub_type` compares the class arguments, so
        // `ScalaType[U]` does not inhabit `ScalaType[A]`.
        (
            Type::Refined {
                parents: ap,
                decls: ad,
            },
            Type::Refined {
                parents: pp,
                decls: pd,
            },
        ) if ad.is_empty() && pd.is_empty() && ap.len() == pp.len() => {
            ap.iter().zip(pp.iter()).all(|(a, p)| {
                matches!(p, Type::TypeParam(_)) || a == p || class_ctor_matches_typeparam_args(a, p)
            })
        }
        (_, Type::TypeParam(_)) => true,
        (a, Type::BoundedWildcard { hi: Some(h), .. }) => class_ctor_matches_typeparam_args(a, h),
        (
            a,
            Type::BoundedWildcard {
                lo: Some(l),
                hi: None,
            },
        ) => class_ctor_matches_typeparam_args(a, l),
        // Last: a *compound* argument inhabits whatever any one of its
        // components does. slick's `MemoryProfile.base[T, U]` hands its
        // `implicitly[BaseColumnType[U]]` -- a `ScalaType[U] with
        // BaseTypedType[U]` -- to `new MappedColumnType(baseType:
        // ColumnType[U'], …)`, whose parameter is the plain `ScalaType[U']`.
        (Type::Refined { parents, .. }, p) => parents
            .iter()
            .any(|a| class_ctor_matches_typeparam_args(a, p)),
        _ => false,
    }
}

pub(crate) fn numeric_widen(a: &Type, b: &Type) -> Option<Type> {
    let a = a.widen_constant();
    let b = b.widen_constant();
    match (&a, &b) {
        (Type::Int, Type::Long) => Some(Type::Long),
        (Type::Int, Type::Double) => Some(Type::Double),
        (Type::Long, Type::Double) => Some(Type::Double),
        (Type::Float, Type::Double) => Some(Type::Double),
        (Type::Int, Type::Float) => Some(Type::Float),
        (Type::Long, Type::Float) => Some(Type::Float),
        // SLS 3.5.3 weak conformance: `Byte <= Short <= Int <= Long <= Float
        // <= Double` and `Char <= Int`. `Byte`/`Short`/`Char` are `int` on the
        // stack, so widening to `Short` or `Int` needs no instruction and
        // `wrap_numeric_widen` just retypes the tree.
        (Type::Byte, Type::Short | Type::Int | Type::Long | Type::Float | Type::Double) => {
            Some(b.clone())
        }
        (Type::Short | Type::Char, Type::Int | Type::Long | Type::Float | Type::Double) => {
            Some(b.clone())
        }
        _ => None,
    }
}

/// Result type of an `if` / `match`: nsc uses the lub of the branches and then
/// adapts to the expected type. We prefer `pt` because a structural lub cannot
/// walk parents (`if (c) Some(1) else None` must stay `Option[Int]`), but only
/// when `pt` really says something. A lambda body is typed against a *stand-in*
/// `Any` whenever the method's result type parameter is still undetermined
/// (`xs.map(f)`'s `B`); adopting it there would make every `if`/`match` bodied
/// lambda infer `A => Any` and collapse `xs.map { case … }` to `List[Any]`.
pub(crate) fn pt_or_lub(pt: &Type, branches: Type) -> Type {
    if !pt.is_no_type() && !matches!(pt, Type::Nothing | Type::Any | Type::TypeParam(_)) {
        pt.clone()
    } else {
        branches
    }
}

/// Whether an expected type is the *nested* form of that same stand-in: an
/// undetermined variable in the result of a function-typed parameter is opened
/// to `Type::Wildcard` rather than to a bound (`check_apply`'s `relaxed`), so
/// `def flatMap[X, Y](fa: F[X])(f: X => F[Y])` hands its literal the expected
/// type `X => F[_]`. `F[_]` is not `Any`, so `pt_or_lub` used to adopt it, and
/// a lambda whose body is an `if` or a `match` came out as `X => F[_]` — the
/// argument that was supposed to *decide* `Y` said `Y = _` instead.
///
/// That is what cats' monad transformers are made of:
/// `EitherT(F.flatMap(value) { case Left(_) => … ; case Right(b) => … })`
/// reported `no matching overload for (F[Either[A, B]])EitherT[F, A, B] with
/// arguments (F[_])` throughout `EitherT`, `OptionT` and `IorT`, while the
/// same body written as a plain lambda (no `match`) type-checked.
pub(crate) fn pt_is_undecided(pt: &Type) -> bool {
    fn walk(t: &Type) -> bool {
        match t {
            Type::Wildcard => true,
            Type::Applied { args, .. } => args.iter().any(walk),
            Type::Class { args, .. } => args.iter().any(walk),
            _ => false,
        }
    }
    // A bare `_` is `pt_or_lub`'s `Any` case already; only an argument
    // position is this stand-in.
    !matches!(pt, Type::Wildcard) && walk(pt)
}

/// Undo the parser's `{A,B=>C,_}` encoding of an import selector list.
/// Each entry is `(name, alias)`; `("_", "_")` is the wildcard, and an alias
/// of `_` hides the name.
pub(crate) fn decode_import_selectors(encoded: &str) -> Vec<(String, String)> {
    let inner = encoded.trim_matches(|c| c == '{' || c == '}');
    let mut out = Vec::new();
    for sel in inner.split(',') {
        let sel = sel.trim();
        if sel.is_empty() {
            continue;
        }
        let (from, to) = match sel.split_once("=>") {
            Some((f, t)) => (f.trim(), t.trim()),
            None => (sel, sel),
        };
        out.push((from.to_string(), to.to_string()));
    }
    out
}

fn import_path(t: &Tree) -> String {
    match &t.kind {
        TreeKind::Ident { name } => name.clone(),
        TreeKind::Select { qual, name } => {
            let p = import_path(qual);
            if p.is_empty() {
                name.clone()
            } else {
                format!("{p}.{name}")
            }
        }
        _ => String::new(),
    }
}

pub(crate) fn import_enables_feature(expr: &Tree, feature: &str) -> bool {
    let p = import_path(expr);
    if p == format!("scala.language.{feature}")
        || p == format!("language.{feature}")
        || p.ends_with(&format!(".language.{feature}"))
        || p == "scala.language._"
        || p == "language._"
        || p.ends_with(".language._")
    {
        return true;
    }
    if let TreeKind::Select { qual, name } = &expr.kind {
        if name.starts_with('{') {
            let qp = import_path(qual);
            let is_lang = qp == "scala.language" || qp == "language" || qp.ends_with(".language");
            if is_lang && (name.contains(feature) || name.contains('_')) {
                return true;
            }
        }
    }
    false
}

fn language_flag_enabled(features: &[String], name: &str) -> bool {
    features.iter().any(|f| f == name || f == "_")
}

pub(crate) fn has_named_dynamic_args(tree: &Tree) -> bool {
    match &tree.kind {
        TreeKind::Apply { args, .. } => args.iter().any(|a| Typer::named_arg_parts(a).is_some()),
        _ => false,
    }
}

/// nsc `isAssignmentOp`: ends with `=`, length > 1, not `==` / `!=` / `<=` / `>=`.
///
/// `scala_rs_parser::ast` exports a function of the same name that answers a
/// *different* question (it also rejects an operator starting with `=`, so
/// `===` is not one). While this lived beside its callers a local item
/// outranked the `use scala_rs_parser::ast::*` glob and the two never met;
/// a caller in another file must name this one explicitly, and does.
pub(crate) fn is_assignment_op(name: &str) -> bool {
    name.len() > 1 && name.ends_with('=') && !matches!(name, "==" | "!=" | "<=" | ">=")
}

pub(crate) fn is_implicit_conversion_shape(vparamss: &[Vec<Tree>]) -> bool {
    let mut n_non_impl = 0usize;
    for clause in vparamss {
        let all_impl = !clause.is_empty()
            && clause.iter().all(|p| match &p.kind {
                TreeKind::ValDef { mods, .. } => mods.flags.contains(Flags::IMPLICIT),
                _ => false,
            });
        if all_impl {
            continue;
        }
        n_non_impl += clause.len();
    }
    n_non_impl == 1
}

/// nsc: `T: C` means implicit evidence of type `C[T]`.
pub(crate) fn is_this_or_super_callee(fun: &Tree) -> bool {
    match &fun.kind {
        TreeKind::This { .. } | TreeKind::Super { .. } => true,
        TreeKind::Ident { name } if name == "this" || name == "super" => true,
        _ => false,
    }
}

/// `this(a)(b)` inside an auxiliary constructor is *one* call.
///
/// A JVM constructor takes a single flat argument list, which is why
/// `extends A(1)(2)` (`type_parent_ctor_app_in`) and `new A(1)(2)`
/// (`flatten_curried_new`) are flattened too. Left nested, the outer list was
/// applied to the `Unit` that `this()` produces, so cats-kernel's
/// `def this(V: Hash[V], O: Order[K], K: Hash[K]) = this()(V, K)` reported
/// three unrelated errors at once: the implicit clause of `this()` could not
/// be filled, `value apply is not a member of Unit`, and -- because the
/// delegation test only looks one `Apply` deep -- `auxiliary constructor must
/// start with a call to this(...)`. nsc types a class whose only clause is
/// implicit as `()(implicit …)`, which is why `this()(V, K)` is written that
/// way in the first place.
pub(crate) fn flatten_curried_ctor_delegation(tree: &mut Tree) {
    let mut depth = 0usize;
    let mut cur: &Tree = tree;
    while let TreeKind::Apply { fun, .. } = &cur.kind {
        depth += 1;
        if is_this_or_super_callee(fun) {
            break;
        }
        cur = fun;
    }
    let head_is_delegation =
        matches!(&cur.kind, TreeKind::Apply { fun, .. } if is_this_or_super_callee(fun));
    if depth < 2 || !head_is_delegation {
        return;
    }
    // The outermost `Apply`'s id and span are what the call is reported and
    // recognised by, so the flattened node keeps them.
    let (id, span) = (tree.id, tree.span);
    let mut argss: Vec<Vec<Tree>> = Vec::new();
    let mut head = std::mem::replace(tree, Tree::dummy(TreeKind::Empty));
    while let TreeKind::Apply { fun, args } = head.kind {
        argss.push(args);
        head = *fun;
    }
    argss.reverse();
    let mut out = Tree::dummy(TreeKind::Apply {
        fun: Box::new(head),
        args: argss.into_iter().flatten().collect(),
    });
    out.id = id;
    out.span = span;
    *tree = out;
}

fn is_ctor_delegation_apply(t: &Tree) -> Option<bool> {
    match &t.kind {
        TreeKind::Apply { fun, .. } => {
            if matches!(&fun.kind, TreeKind::Super { .. })
                || matches!(&fun.kind, TreeKind::Ident { name } if name == "super")
            {
                Some(true)
            } else if matches!(&fun.kind, TreeKind::This { .. })
                || matches!(&fun.kind, TreeKind::Ident { name } if name == "this")
            {
                Some(false)
            } else {
                None
            }
        }
        TreeKind::Typed { expr, .. } => is_ctor_delegation_apply(expr),
        _ => None,
    }
}

fn tree_has_ctor_delegation(t: &Tree) -> bool {
    if is_ctor_delegation_apply(t).is_some() {
        return true;
    }
    match &t.kind {
        TreeKind::Block { stats, expr } => {
            stats.iter().any(tree_has_ctor_delegation) || tree_has_ctor_delegation(expr)
        }
        TreeKind::Typed { expr, .. } => tree_has_ctor_delegation(expr),
        _ => false,
    }
}

pub(crate) fn first_ctor_delegation(rhs: &Tree) -> CtorDelegation {
    match &rhs.kind {
        TreeKind::Typed { expr, .. } => first_ctor_delegation(expr),
        TreeKind::Block { stats, expr } => {
            let first = stats.first().unwrap_or(expr);
            match is_ctor_delegation_apply(first) {
                Some(true) => CtorDelegation::Super,
                Some(false) => CtorDelegation::This,
                None => {
                    if tree_has_ctor_delegation(rhs) {
                        CtorDelegation::AfterStats
                    } else {
                        CtorDelegation::Missing
                    }
                }
            }
        }
        _ => match is_ctor_delegation_apply(rhs) {
            Some(true) => CtorDelegation::Super,
            Some(false) => CtorDelegation::This,
            None => {
                if tree_has_ctor_delegation(rhs) {
                    CtorDelegation::AfterStats
                } else {
                    CtorDelegation::Missing
                }
            }
        },
    }
}

pub(crate) fn scala_module_evidence_type(outer: SymbolId, simple: &str) -> Option<Type> {
    if outer.is_none() {
        return None;
    }
    let arg = match simple {
        "Int" => Type::Int,
        "Long" => Type::Long,
        "Double" => Type::Double,
        "Float" => Type::Float,
        "Boolean" => Type::Boolean,
        "Byte" => Type::Byte,
        "Short" => Type::Short,
        "Char" => Type::Char,
        "Unit" => Type::Unit,
        "String" => Type::String,
        _ => return None,
    };
    Some(Type::Class {
        sym: outer,
        args: vec![arg],
    })
}

pub(crate) fn mark_nested_module_implicit(
    st: &mut SymbolTable,
    owner: SymbolId,
    simple: &str,
    ev: Type,
) {
    let Some(m) = st
        .lookup_member(owner, simple)
        .into_iter()
        .find(|&s| st.get(s).kind == SymKind::Module)
    else {
        return;
    };
    let f = st.get(m).flags.with(Flags::IMPLICIT);
    st.get_mut(m).flags = f;
    let cls = st.module_class_of(m);
    st.get_mut(cls).parents = vec![ev];
}

pub(crate) fn apply_context_bound(bound: Type, tp: SymbolId) -> Type {
    match bound {
        Type::Class { sym, args } if args.is_empty() => Type::Class {
            sym,
            args: vec![Type::TypeParam(tp)],
        },
        Type::Named { name, args } if args.is_empty() => Type::Named {
            name,
            args: vec![Type::TypeParam(tp)],
        },
        // `U: BaseColumnType` where `BaseColumnType` is a *parameterized type
        // member* (or another type parameter) still means `BaseColumnType[U]`.
        bound @ (Type::TypeMember(_) | Type::TypeParam(_)) => {
            crate::symbol::apply_type_ctor(bound, vec![Type::TypeParam(tp)])
        }
        other => other,
    }
}

pub(crate) fn array_elem_of(ty: &Type) -> Option<Type> {
    match ty {
        Type::Array(t) => Some((**t).clone()),
        Type::Named { name, args } if name == "Array" && !args.is_empty() => Some(args[0].clone()),
        Type::Class { args, .. } if args.len() == 1 => {
            // only when the class is Array; callers pass New's type which is Array(_)
            None
        }
        _ => None,
    }
}

/// Expand type aliases. `alias_ids` are `type T = …` (non-empty rhs).
/// Hitting an alias already on `seen` is `illegal cyclic reference`.
pub(crate) fn expand_alias_type(
    st: &SymbolTable,
    ty: &Type,
    alias_ids: &HashSet<u32>,
    seen: &mut Vec<u32>,
) -> Result<Type, SymbolId> {
    match ty {
        Type::TypeMember(id) => {
            if !st.get(*id).tparams.is_empty() {
                return Ok(Type::TypeMember(*id));
            }
            let rhs = st.get(*id).ty.clone();
            match &rhs {
                Type::NoType | Type::Error => Ok(Type::TypeMember(*id)),
                Type::TypeMember(x) if *x == *id => {
                    if alias_ids.contains(&id.0) {
                        Err(*id)
                    } else {
                        Ok(Type::TypeMember(*id))
                    }
                }
                other => {
                    if seen.contains(&id.0) {
                        return Err(*id);
                    }
                    seen.push(id.0);
                    let r = expand_alias_type(st, other, alias_ids, seen);
                    seen.pop();
                    r
                }
            }
        }
        Type::Class { sym, args } => {
            let args: Result<Vec<_>, _> = args
                .iter()
                .map(|a| expand_alias_type(st, a, alias_ids, seen))
                .collect();
            Ok(Type::Class {
                sym: *sym,
                args: args?,
            })
        }
        Type::Applied { ctor, args } => {
            let ctor = expand_alias_type(st, ctor, alias_ids, seen)?;
            let args: Result<Vec<_>, _> = args
                .iter()
                .map(|a| expand_alias_type(st, a, alias_ids, seen))
                .collect();
            Ok(st.expand_applied_hk_alias(crate::symbol::apply_type_ctor(ctor, args?)))
        }
        Type::Array(t) => Ok(Type::Array(Box::new(expand_alias_type(
            st, t, alias_ids, seen,
        )?))),
        Type::Function { params, ret } => {
            let params: Result<Vec<_>, _> = params
                .iter()
                .map(|p| expand_alias_type(st, p, alias_ids, seen))
                .collect();
            Ok(Type::Function {
                params: params?,
                ret: Box::new(expand_alias_type(st, ret, alias_ids, seen)?),
            })
        }
        Type::Tuple(ts) => {
            let ts: Result<Vec<_>, _> = ts
                .iter()
                .map(|t| expand_alias_type(st, t, alias_ids, seen))
                .collect();
            Ok(Type::Tuple(ts?))
        }
        Type::Named { name, args } => {
            let args: Result<Vec<_>, _> = args
                .iter()
                .map(|a| expand_alias_type(st, a, alias_ids, seen))
                .collect();
            Ok(Type::Named {
                name: name.clone(),
                args: args?,
            })
        }
        Type::Refined { parents, decls } => {
            let parents: Result<Vec<_>, _> = parents
                .iter()
                .map(|p| expand_alias_type(st, p, alias_ids, seen))
                .collect();
            Ok(Type::Refined {
                parents: parents?,
                decls: decls.clone(),
            })
        }
        Type::Annotated { tpe, annot } => Ok(Type::Annotated {
            tpe: Box::new(expand_alias_type(st, tpe, alias_ids, seen)?),
            annot: annot.clone(),
        }),
        other => Ok(other.clone()),
    }
}

pub(crate) fn needs_classtag_elem(elem: &Type) -> bool {
    matches!(
        elem,
        Type::TypeParam(_) | Type::TypeMember(_) | Type::Wildcard
    )
}

/// Copy a type-parameter list for a synthesized definition.
///
/// The copy must own its symbols: `enter_tparams` reuses whatever `sym` a
/// `TypeDef` already carries, so a straight `clone` would make the synthetic
/// method's parameters *be* the class's, owned by the wrong symbol. Higher
/// kinded parameters (`F[_]`) carry nested `TypeDef`s that get symbols too, so
/// the reset recurses through them. Bound trees are read by `tree_to_type`,
/// which never writes back into the tree, so they can be shared as cloned.
fn copy_tparams(tparams: &[Tree]) -> Vec<Tree> {
    tparams
        .iter()
        .map(|tp| {
            let mut c = tp.clone();
            c.id = NodeId(0);
            c.sym = SymbolId::NONE;
            c.ty = Type::NoType;
            if let TreeKind::TypeDef { tparams: inner, .. } = &mut c.kind {
                *inner = copy_tparams(inner);
            }
            c
        })
        .collect()
}

pub(crate) fn implicit_class_conversions(body: &[Tree]) -> Vec<Tree> {
    let mut out = Vec::new();
    for stt in body {
        let TreeKind::ClassDef {
            mods,
            name,
            vparamss,
            tparams,
            ..
        } = &stt.kind
        else {
            continue;
        };
        if !mods.flags.contains(Flags::IMPLICIT) {
            continue;
        }
        let Some(params) = vparamss.first() else {
            continue;
        };
        if params.len() != 1 {
            continue;
        }
        let p = &params[0];
        let pname = p.name().unwrap_or("x$0").to_string();
        let tpt = match &p.kind {
            TreeKind::ValDef { tpt, .. } => (**tpt).clone(),
            _ => continue,
        };
        let mut param = Tree::dummy(TreeKind::ValDef {
            mods: Modifiers::new(Flags::PARAM.with(Flags::SYNTHETIC)),
            name: pname.clone(),
            tpt: Box::new(tpt),
            rhs: Box::new(Tree::dummy(TreeKind::Empty)),
        });
        param.span = p.span;
        // nsc: `implicit class C[T <: B](x: P)` desugars to
        // `implicit def C[T <: B](x: P): C[T] = new C[T](x)`. Dropping the
        // type parameters would leave a bare `C` behind -- a type constructor
        // where a proper type is required.
        let conv_tparams = copy_tparams(tparams);
        let tparam_refs: Vec<Tree> = conv_tparams
            .iter()
            .map(|tp| Tree {
                id: NodeId(0),
                span: stt.span,
                kind: TreeKind::Ident {
                    name: tp.name().unwrap_or("_").to_string(),
                },
                ty: Type::NoType,
                sym: SymbolId::NONE,
                postfix: false,
                scala_ref: false,
                stable_pat: false,
            })
            .collect();
        let cls_ident = |sym| Tree {
            id: NodeId(0),
            span: stt.span,
            kind: TreeKind::Ident { name: name.clone() },
            ty: Type::NoType,
            sym,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
        };
        // `C` when the class is monomorphic, `C[T1, .., Tn]` otherwise.
        let cls_type = |sym| {
            if tparam_refs.is_empty() {
                cls_ident(sym)
            } else {
                Tree {
                    id: NodeId(0),
                    span: stt.span,
                    kind: TreeKind::AppliedTypeTree {
                        tpt: Box::new(cls_ident(sym)),
                        args: tparam_refs.clone(),
                    },
                    ty: Type::NoType,
                    sym: SymbolId::NONE,
                    postfix: false,
                    scala_ref: false,
                    stable_pat: false,
                }
            }
        };
        let cls_tpt = cls_type(stt.sym);
        let arg = Tree {
            id: NodeId(0),
            span: p.span,
            kind: TreeKind::Ident { name: pname },
            ty: Type::NoType,
            sym: SymbolId::NONE,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
        };
        let rhs = Tree {
            id: NodeId(0),
            span: stt.span,
            kind: TreeKind::Apply {
                fun: Box::new(Tree {
                    id: NodeId(0),
                    span: stt.span,
                    kind: TreeKind::New {
                        tpt: Box::new(cls_tpt),
                    },
                    ty: Type::NoType,
                    sym: stt.sym,
                    postfix: false,
                    scala_ref: false,
                    stable_pat: false,
                }),
                args: vec![arg],
            },
            ty: Type::NoType,
            sym: SymbolId::NONE,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
        };
        // nsc keeps the class's *remaining* clauses on the conversion and
        // passes them straight through:
        // `implicit class Ops[A](a: A)(implicit s: Show[A])` desugars to
        // `implicit def Ops[A](a: A)(implicit s: Show[A]): Ops[A] =
        //    new Ops[A](a)(s)`.
        // Dropping them left `new Ops[A](a)` to summon a `Show[A]` for an
        // abstract `A` inside the conversion, which is "could not find
        // implicit value of type Show[A]" *at the class declaration* -- an
        // error real scalac never reports, and it took every type-class
        // syntax class (`cats`' `x.show`, `fa.map`) with it.
        let mut vparamss_conv = vec![vec![param]];
        let mut rhs = rhs;
        for clause in vparamss.iter().skip(1) {
            let mut decls = Vec::new();
            let mut args = Vec::new();
            for q in clause {
                let TreeKind::ValDef {
                    mods: qm, tpt: qt, ..
                } = &q.kind
                else {
                    continue;
                };
                let qname = q.name().unwrap_or("x$0").to_string();
                let mut decl = Tree::dummy(TreeKind::ValDef {
                    mods: Modifiers::new(qm.flags.with(Flags::SYNTHETIC)),
                    name: qname.clone(),
                    tpt: (*qt).clone(),
                    rhs: Box::new(Tree::dummy(TreeKind::Empty)),
                });
                decl.span = q.span;
                decls.push(decl);
                args.push(Tree {
                    id: NodeId(0),
                    span: q.span,
                    kind: TreeKind::Ident { name: qname },
                    ty: Type::NoType,
                    sym: SymbolId::NONE,
                    postfix: false,
                    scala_ref: false,
                    stable_pat: false,
                });
            }
            if decls.is_empty() {
                continue;
            }
            rhs = Tree {
                id: NodeId(0),
                span: stt.span,
                kind: TreeKind::Apply {
                    fun: Box::new(rhs),
                    args,
                },
                ty: Type::NoType,
                sym: SymbolId::NONE,
                postfix: false,
                scala_ref: false,
                stable_pat: false,
            };
            vparamss_conv.push(decls);
        }
        let mut conv = Tree::dummy(TreeKind::DefDef {
            mods: Modifiers::new(Flags::IMPLICIT.with(Flags::SYNTHETIC)),
            name: name.clone(),
            tparams: conv_tparams,
            vparamss: vparamss_conv,
            tpt: Box::new(cls_type(stt.sym)),
            rhs: Box::new(rhs),
        });
        conv.span = stt.span;
        out.push(conv);
    }
    out
}

/// Every parameter of a member, clauses flattened. A `val`/`var` (any
/// non-method type) is parameterless, like a nullary `def`.
pub(crate) fn flat_param_types(ty: &Type) -> Vec<Type> {
    match ty {
        Type::Method { paramss, .. } => paramss.iter().flatten().cloned().collect(),
        _ => Vec::new(),
    }
}

/// Whether `ty` mentions a type parameter or an abstract type member, i.e.
/// whether it reads differently depending on the prefix it is seen from.
pub(crate) fn sig_has_abstract_type(ty: &Type) -> bool {
    match ty {
        Type::TypeParam(_) | Type::TypeMember(_) => true,
        Type::Class { args, .. } | Type::Named { args, .. } | Type::Tuple(args) => {
            args.iter().any(sig_has_abstract_type)
        }
        Type::Applied { ctor, args } => {
            sig_has_abstract_type(ctor) || args.iter().any(sig_has_abstract_type)
        }
        Type::Array(t) | Type::ByName(t) | Type::Repeated(t) | Type::Annotated { tpe: t, .. } => {
            sig_has_abstract_type(t)
        }
        Type::Function { params, ret } => {
            params.iter().any(sig_has_abstract_type) || sig_has_abstract_type(ret)
        }
        _ => false,
    }
}

pub(crate) fn split_repeated(params: &[Type]) -> (&[Type], Option<&Type>) {
    match params.last() {
        Some(Type::Repeated(t)) => (&params[..params.len() - 1], Some(t.as_ref())),
        _ => (params, None),
    }
}

pub(crate) fn param_at(params: &[Type], i: usize) -> Option<&Type> {
    let (fixed, repeated) = split_repeated(params);
    if i < fixed.len() {
        Some(&fixed[i])
    } else {
        repeated
    }
}

/// Whether `ty` still mentions a type parameter, i.e. is not a proper type yet.
pub(crate) fn mentions_any_tparam(ty: &Type) -> bool {
    match ty {
        Type::TypeParam(_) => true,
        Type::Class { args, .. } | Type::Tuple(args) => args.iter().any(mentions_any_tparam),
        Type::Applied { ctor, args } => {
            mentions_any_tparam(ctor) || args.iter().any(mentions_any_tparam)
        }
        Type::Array(t) | Type::ByName(t) | Type::Repeated(t) | Type::Annotated { tpe: t, .. } => {
            mentions_any_tparam(t)
        }
        Type::Function { params, ret } => {
            params.iter().any(mentions_any_tparam) || mentions_any_tparam(ret)
        }
        _ => false,
    }
}

pub(crate) fn flip_variance(v: i8) -> i8 {
    -v
}

/// Variance of an occurrence nested `inner` deep inside an `outer` position.
/// An invariant position stays invariant however it is nested.
pub(crate) fn compose_variance(outer: i8, inner: i8) -> i8 {
    if outer == 0 || inner == 0 {
        0
    } else {
        outer * inner
    }
}

/// `scala.Array` reached through a classfile signature arrives as
/// `Class { sym: array_sym }`, whose JVM name is the pseudo-name
/// `[java/lang/Object`. Anything used as an inferred type argument has to be
/// in `Type::Array` form or the backend emits a method owner that no JVM can
/// load.
pub(crate) fn unarrayify(t: &Type, array_sym: SymbolId) -> Type {
    match t {
        Type::Class { sym, args } if *sym == array_sym && args.len() == 1 => {
            Type::Array(Box::new(unarrayify(&args[0], array_sym)))
        }
        Type::Class { sym, args } if !args.is_empty() => Type::Class {
            sym: *sym,
            args: args.iter().map(|a| unarrayify(a, array_sym)).collect(),
        },
        Type::Array(e) => Type::Array(Box::new(unarrayify(e, array_sym))),
        Type::Tuple(ts) => Type::Tuple(ts.iter().map(|a| unarrayify(a, array_sym)).collect()),
        Type::Function { params, ret } => Type::Function {
            params: params.iter().map(|a| unarrayify(a, array_sym)).collect(),
            ret: Box::new(unarrayify(ret, array_sym)),
        },
        other => other.clone(),
    }
}

/// The first of `params` that pins `tp` against the matching `args` entry.
pub(crate) fn unify_tparam(tp: SymbolId, params: &[Type], args: &[Type]) -> Option<Type> {
    for (p, a) in params.iter().zip(args) {
        if let Some(t) = unify_one(tp, p, a) {
            return Some(t);
        }
    }
    None
}

/// True when `args` instantiate `tps` rather than still mentioning them
/// (`Inv[A @uncheckedVariance]` is not an instantiation of `Inv`).
pub(crate) fn type_args_are_instantiated(args: &[Type], tps: &[SymbolId]) -> bool {
    !args.is_empty()
        && (tps.is_empty() || args.len() == tps.len())
        && args.iter().all(|a| !still_raw_tparam(a, tps))
}

/// A function literal whose parameter types are not written out. Its parameters
/// can only come from the expected type.
pub(crate) fn is_bare_lambda(t: &Tree) -> bool {
    matches!(t.kind, TreeKind::Function { .. }) && !is_annotated_lambda(t)
}

/// Whether `ty` is, or contains, an unknown type. A lambda argument that has
/// not been typed yet is carried as `(<notype>) => <notype>`.
/// A type with nothing left to solve: no `<notype>`, no wildcard, no type
/// parameter or abstract type member. Only such a pair may be compared
/// strictly when scoring a function argument against a function parameter.
pub(crate) fn is_rigid_type(ty: &Type) -> bool {
    match ty {
        Type::NoType
        | Type::Wildcard
        | Type::BoundedWildcard { .. }
        | Type::TypeParam(_)
        | Type::TypeMember(_)
        | Type::Error => false,
        Type::Class { args, .. } | Type::Named { args, .. } | Type::Tuple(args) => {
            args.iter().all(is_rigid_type)
        }
        Type::Applied { ctor, args } => is_rigid_type(ctor) && args.iter().all(is_rigid_type),
        Type::Function { params, ret } => params.iter().all(is_rigid_type) && is_rigid_type(ret),
        Type::Array(t) | Type::ByName(t) | Type::Repeated(t) | Type::Annotated { tpe: t, .. } => {
            is_rigid_type(t)
        }
        Type::Method { paramss, ret } => {
            paramss.iter().flatten().all(is_rigid_type) && is_rigid_type(ret)
        }
        _ => true,
    }
}

/// The object an indexing applies to. `type_expr` has already rewritten `t(i)`
/// to `t.apply(i)`, exactly as nsc does, and nsc's `convertToAssignment` takes
/// its `mkUpdate` branch only for that shape
/// (`treeInfo.Applied(Select(table, nme.apply), _, _)`): `foo.bar(0) += 1`,
/// where `bar` is an ordinary method, is an error there, since rewriting it to
/// `foo.bar.update(0, …)` would drop `bar`'s own argument list.
pub(crate) fn index_table(callee: &Tree) -> Option<&Tree> {
    match &callee.kind {
        TreeKind::Select { qual, name } if name == "apply" => Some(qual),
        TreeKind::TypeApply { fun, .. } => index_table(fun),
        _ => None,
    }
}

/// Clear what the typer wrote on a tree that is about to be re-typed in a new
/// position, so it starts from the name again. `arr` inside `arr(0)` is left
/// holding the array's `apply` *method* type, and moving that into
/// `arr.update(…)` looked `update` up on `(Int)Int`. Ids are kept:
/// `type_apply` recognises the arguments it filled in itself by id and drops
/// them before resolving the call again.
pub(crate) fn reset_for_retyping(c: &mut Tree) {
    c.sym = SymbolId::NONE;
    c.ty = Type::NoType;
    match &mut c.kind {
        TreeKind::Select { qual, .. } => reset_for_retyping(qual),
        TreeKind::Apply { fun, args } | TreeKind::TypeApply { fun, args } => {
            reset_for_retyping(fun);
            for a in args.iter_mut() {
                reset_for_retyping(a);
            }
        }
        TreeKind::Typed { expr, .. } => reset_for_retyping(expr),
        _ => {}
    }
}

/// The type arguments the user wrote on the callee, if the callee is a
/// `TypeApply`. `type_expr` has already resolved each argument tree's `ty`.
pub(crate) fn explicit_type_args(fun: &Tree) -> Option<Vec<Type>> {
    match &fun.kind {
        TreeKind::TypeApply { args, .. } => {
            let targs: Vec<Type> = args.iter().map(|a| a.ty.clone()).collect();
            (!targs.is_empty() && !targs.iter().any(|t| t.is_no_type() || t.is_error()))
                .then_some(targs)
        }
        _ => None,
    }
}

pub(crate) fn mentions_no_type(ty: &Type) -> bool {
    match ty {
        Type::NoType => true,
        Type::Class { args, .. } | Type::Named { args, .. } | Type::Tuple(args) => {
            args.iter().any(mentions_no_type)
        }
        Type::Applied { ctor, args } => mentions_no_type(ctor) || args.iter().any(mentions_no_type),
        Type::Function { params, ret } => {
            params.iter().any(mentions_no_type) || mentions_no_type(ret)
        }
        Type::Array(t) | Type::ByName(t) | Type::Repeated(t) | Type::Annotated { tpe: t, .. } => {
            mentions_no_type(t)
        }
        Type::Method { paramss, ret } => {
            paramss.iter().flatten().any(mentions_no_type) || mentions_no_type(ret)
        }
        _ => false,
    }
}

/// Whether `ty` mentions any of the method type parameters in `tps`.
pub(crate) fn mentions_tparam(ty: &Type, tps: &[SymbolId]) -> bool {
    match ty {
        Type::TypeParam(id) => tps.contains(id),
        Type::Class { args, .. } => args.iter().any(|a| mentions_tparam(a, tps)),
        Type::Applied { ctor, args } => {
            mentions_tparam(ctor, tps) || args.iter().any(|a| mentions_tparam(a, tps))
        }
        Type::Function { params, ret } => {
            params.iter().any(|p| mentions_tparam(p, tps)) || mentions_tparam(ret, tps)
        }
        Type::Array(e) | Type::ByName(e) | Type::Repeated(e) => mentions_tparam(e, tps),
        Type::Annotated { tpe, .. } => mentions_tparam(tpe, tps),
        Type::Tuple(ts) => ts.iter().any(|t| mentions_tparam(t, tps)),
        _ => false,
    }
}

/// Every type parameter `ty` mentions, in order of first appearance.
pub(crate) fn collect_tparams(ty: &Type, out: &mut Vec<SymbolId>) {
    match ty {
        Type::TypeParam(id) => {
            if !out.contains(id) {
                out.push(*id);
            }
        }
        Type::Class { args, .. } | Type::Tuple(args) | Type::Named { args, .. } => {
            for a in args {
                collect_tparams(a, out);
            }
        }
        Type::Applied { ctor, args } => {
            collect_tparams(ctor, out);
            for a in args {
                collect_tparams(a, out);
            }
        }
        Type::Array(t) | Type::ByName(t) | Type::Repeated(t) | Type::Annotated { tpe: t, .. } => {
            collect_tparams(t, out)
        }
        Type::Function { params, ret } => {
            for p in params {
                collect_tparams(p, out);
            }
            collect_tparams(ret, out);
        }
        Type::Method { paramss, ret } => {
            for ps in paramss {
                for p in ps {
                    collect_tparams(p, out);
                }
            }
            collect_tparams(ret, out);
        }
        _ => {}
    }
}

fn still_raw_tparam(ty: &Type, tps: &[SymbolId]) -> bool {
    match ty {
        Type::TypeParam(id) => tps.contains(id),
        Type::Applied { ctor, args } => {
            still_raw_tparam(ctor, tps) || args.iter().any(|a| still_raw_tparam(a, tps))
        }
        Type::Annotated { tpe, .. } => still_raw_tparam(tpe, tps),
        _ => false,
    }
}

/// Whether `tp` occurs anywhere in `ty`.
/// Whether a wildcard occurs anywhere in `ty` -- i.e. whether it is a type the
/// expected-type relaxation put there rather than one the source wrote.
pub(crate) fn type_mentions_wildcard(ty: &Type) -> bool {
    match ty {
        Type::Wildcard | Type::BoundedWildcard { .. } => true,
        Type::Class { args, .. } | Type::Named { args, .. } | Type::Tuple(args) => {
            args.iter().any(type_mentions_wildcard)
        }
        Type::Applied { ctor, args } => {
            type_mentions_wildcard(ctor) || args.iter().any(type_mentions_wildcard)
        }
        Type::Array(t) | Type::ByName(t) | Type::Repeated(t) | Type::Annotated { tpe: t, .. } => {
            type_mentions_wildcard(t)
        }
        Type::Function { params, ret } => {
            params.iter().any(type_mentions_wildcard) || type_mentions_wildcard(ret)
        }
        Type::Refined { parents, .. } => parents.iter().any(type_mentions_wildcard),
        _ => false,
    }
}

pub(crate) fn type_mentions_tparam(ty: &Type, tp: SymbolId) -> bool {
    match ty {
        Type::TypeParam(id) => *id == tp,
        Type::Class { args, .. } | Type::Named { args, .. } | Type::Tuple(args) => {
            args.iter().any(|t| type_mentions_tparam(t, tp))
        }
        Type::Applied { ctor, args } => {
            type_mentions_tparam(ctor, tp) || args.iter().any(|t| type_mentions_tparam(t, tp))
        }
        Type::Array(t) | Type::ByName(t) | Type::Repeated(t) | Type::Annotated { tpe: t, .. } => {
            type_mentions_tparam(t, tp)
        }
        Type::Function { params, ret } => {
            params.iter().any(|t| type_mentions_tparam(t, tp)) || type_mentions_tparam(ret, tp)
        }
        Type::Method { paramss, ret } => {
            paramss
                .iter()
                .flatten()
                .any(|t| type_mentions_tparam(t, tp))
                || type_mentions_tparam(ret, tp)
        }
        _ => false,
    }
}

/// [`type_mentions_tparam`], but also inside a refinement's parents and
/// declarations.
///
/// The shallow one deliberately stops at a compound type -- see
/// `adapt_implicit_apply`, where looking inside would start a search at an
/// unsubstituted parameter (fixture `ovl4`). A refinement's *declarations* are
/// where cats puts the parameter that only the witness can pin down:
/// `type Aux[M[_], F0[_]] = Parallel[M] { type F[x] = F0[x] }`, and
/// `parUnorderedSequence[T, M, F, A](ta: T[M[A]])(implicit P: Parallel.Aux[M, F])`
/// mentions `F` nowhere else.
pub(crate) fn type_mentions_tparam_deep(ty: &Type, tp: SymbolId) -> bool {
    if type_mentions_tparam(ty, tp) {
        return true;
    }
    let decl_types = |d: &scala_rs_parser::RefineDecl| -> Vec<Type> {
        match d {
            scala_rs_parser::RefineDecl::Type { rhs, lo, hi, .. } => {
                [rhs, lo, hi].iter().filter_map(|t| (*t).clone()).collect()
            }
            scala_rs_parser::RefineDecl::Def { paramss, ret, .. } => paramss
                .iter()
                .flatten()
                .cloned()
                .chain(std::iter::once(ret.clone()))
                .collect(),
            scala_rs_parser::RefineDecl::Val { ty, .. } => vec![ty.clone()],
        }
    };
    match ty {
        Type::Refined { parents, decls } => {
            parents.iter().any(|p| type_mentions_tparam_deep(p, tp))
                || decls
                    .iter()
                    .flat_map(decl_types)
                    .any(|t| type_mentions_tparam_deep(&t, tp))
        }
        Type::Class { args, .. } | Type::Named { args, .. } | Type::Tuple(args) => {
            args.iter().any(|t| type_mentions_tparam_deep(t, tp))
        }
        Type::Applied { ctor, args } => {
            type_mentions_tparam_deep(ctor, tp)
                || args.iter().any(|t| type_mentions_tparam_deep(t, tp))
        }
        Type::Array(t) | Type::ByName(t) | Type::Repeated(t) | Type::Annotated { tpe: t, .. } => {
            type_mentions_tparam_deep(t, tp)
        }
        Type::Function { params, ret } => {
            params.iter().any(|t| type_mentions_tparam_deep(t, tp))
                || type_mentions_tparam_deep(ret, tp)
        }
        Type::Method { paramss, ret } => {
            paramss
                .iter()
                .flatten()
                .any(|t| type_mentions_tparam_deep(t, tp))
                || type_mentions_tparam_deep(ret, tp)
        }
        _ => false,
    }
}

pub(crate) fn unify_one(tp: SymbolId, pattern: &Type, actual: &Type) -> Option<Type> {
    if let Type::Annotated { tpe, .. } = actual {
        return unify_one(tp, pattern, tpe);
    }
    match pattern {
        Type::Annotated { tpe, .. } => unify_one(tp, tpe, actual),
        Type::TypeParam(id) if *id == tp => {
            if actual.is_no_type() || actual.is_error() {
                None
            } else {
                Some(actual.widen_constant())
            }
        }
        Type::BoundedWildcard { hi: Some(h), .. } | Type::BoundedWildcard { lo: Some(h), .. } => {
            unify_one(tp, h, actual)
        }
        Type::Wildcard => None,
        Type::Class { args: pas, .. } => {
            // `Tuple2[K, V]` against `(Int, String)`: the tuple sugar and the
            // nominal class denote the same type (`is_sub_type` already treats
            // them as such), so unify positionally when the arity agrees.
            let aas = match actual {
                Type::Class { args, .. } => args,
                Type::Tuple(ts) if ts.len() == pas.len() => ts,
                // A *compound* actual is each of its components: slick hands a
                // `ScalaType[U] with BaseTypedType[U]` to a `ColumnType[U']`
                // (= `ScalaType[U']`) parameter, and nothing else says what
                // `U'` is.
                Type::Refined { parents, .. } => {
                    for a in parents {
                        if let Some(t) = unify_one(tp, pattern, a) {
                            return Some(t);
                        }
                    }
                    return None;
                }
                _ => return None,
            };
            for (p, a) in pas.iter().zip(aas) {
                if let Some(t) = unify_one(tp, p, a) {
                    return Some(t);
                }
            }
            None
        }
        // `Show[(A, B)]` against `Show[Tuple2[Int, String]]`.
        Type::Tuple(pts) => {
            let aas = match actual {
                Type::Tuple(ts) if ts.len() == pts.len() => ts,
                Type::Class { args, .. } if args.len() == pts.len() => args,
                _ => return None,
            };
            for (p, a) in pts.iter().zip(aas) {
                if let Some(t) = unify_one(tp, p, a) {
                    return Some(t);
                }
            }
            None
        }
        Type::Applied { ctor, args: pas } => match actual {
            Type::Applied {
                ctor: ac,
                args: aas,
            } => {
                if let Some(t) = unify_one(tp, ctor, ac) {
                    return Some(t);
                }
                for (p, a) in pas.iter().zip(aas) {
                    if let Some(t) = unify_one(tp, p, a) {
                        return Some(t);
                    }
                }
                None
            }
            Type::Class { sym, args: aas } => {
                let unapplied = Type::Class {
                    sym: *sym,
                    args: vec![],
                };
                if let Some(t) = unify_one(tp, ctor, &unapplied) {
                    return Some(t);
                }
                for (p, a) in pas.iter().zip(aas) {
                    if let Some(t) = unify_one(tp, p, a) {
                        return Some(t);
                    }
                }
                None
            }
            _ => None,
        },
        Type::Function { params, ret } => {
            if let Type::Function {
                params: aps,
                ret: ar,
            } = actual
            {
                for (p, a) in params.iter().zip(aps) {
                    if let Some(t) = unify_one(tp, p, a) {
                        return Some(t);
                    }
                }
                unify_one(tp, ret, ar)
            } else {
                None
            }
        }
        // `def f[A](t: X[A] with Y[A])` against an `X[U] with Y[U]`: nothing
        // but the compound's components says what `A` is. slick writes
        // `type BaseColumnType[T] = ScalaType[T] with BaseTypedType[T]` and
        // passes `implicitly[BaseColumnType[U]]` to
        // `assertNonNullType[A](t: BaseColumnType[A])`. Components are paired
        // by position (both sides come from the same alias whenever this
        // fires); a non-compound actual is tried against every component, the
        // way a subtype of the compound arrives.
        Type::Refined { parents, .. } => {
            match actual {
                Type::Refined { parents: aps, .. } if aps.len() == parents.len() => {
                    for (p, a) in parents.iter().zip(aps) {
                        if let Some(t) = unify_one(tp, p, a) {
                            return Some(t);
                        }
                    }
                }
                _ => {
                    for p in parents {
                        if let Some(t) = unify_one(tp, p, actual) {
                            return Some(t);
                        }
                    }
                }
            }
            None
        }
        Type::Array(p) => match actual {
            Type::Array(a) => unify_one(tp, p, a),
            _ => None,
        },
        Type::ByName(p) => match actual {
            Type::ByName(a) => unify_one(tp, p, a),
            _ => unify_one(tp, p, actual),
        },
        // `Seq(xs: _*)` hands the parameter a `Repeated` of its *element*
        // type, not of the sequence: unwrapping only the pattern solved
        // `Seq.apply[A](A*)` to `A = Int*` and made `Seq(xs: _*)` a
        // `Seq[Int*]`.
        Type::Repeated(p) => match actual {
            Type::Repeated(a) => unify_one(tp, p, a),
            _ => unify_one(tp, p, actual),
        },
        _ => None,
    }
}

impl Typer {
    pub fn dump_typed(&self, tree: &Tree) -> String {
        scala_rs_parser::dump_tree(tree)
    }
}

/// Whether a compilation unit defines `def main(args: Array[String])` on an object.
pub fn find_mains(st: &SymbolTable, tree: &Tree) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(st: &SymbolTable, t: &Tree, out: &mut Vec<String>) {
        match &t.kind {
            TreeKind::PackageDef { stats, .. } => {
                for s in stats {
                    walk(st, s, out);
                }
            }
            TreeKind::ModuleDef { name, impl_, .. } => {
                let mut has_main = false;
                for b in &impl_.body {
                    if let TreeKind::DefDef { name: mn, .. } = &b.kind {
                        if mn == "main" {
                            has_main = true;
                        }
                    }
                }
                if !has_main {
                    has_main = impl_.parents.iter().any(|p| parent_is_app(st, p));
                }
                if has_main {
                    out.push(name.clone());
                }
            }
            TreeKind::ClassDef { impl_, .. } => {
                for b in &impl_.body {
                    walk(st, b, out);
                }
            }
            _ => {}
        }
    }
    walk(st, tree, &mut out);
    out
}

fn parent_is_app(st: &SymbolTable, p: &Tree) -> bool {
    let id = st
        .class_sym_of(&p.ty)
        .or_else(|| if p.sym.is_none() { None } else { Some(p.sym) });
    let Some(id) = id else {
        return p.name() == Some("App");
    };
    class_extends_named(st, id, "App")
}

fn class_extends_named(st: &SymbolTable, id: SymbolId, name: &str) -> bool {
    if st.get(id).name == name {
        return true;
    }
    let mut work = st.get(id).parents.clone();
    let mut seen = std::collections::HashSet::new();
    seen.insert(id.0);
    while let Some(p) = work.pop() {
        let Some(pid) = st.class_sym_of(&p) else {
            continue;
        };
        if !seen.insert(pid.0) {
            continue;
        }
        if st.get(pid).name == name {
            return true;
        }
        work.extend(st.get(pid).parents.clone());
    }
    false
}

/// Replace quantified existential names (`type X` / `type X <: Bound`) with
/// wildcards. Bounded forms become `BoundedWildcard` so pickle/erasure reuse
/// the `List[_ <: AnyRef]` path.
pub(crate) struct ExistQuant {
    pub(crate) name: String,
    pub(crate) lo: Option<Type>,
    pub(crate) hi: Option<Type>,
}

pub(crate) fn subst_quantified(ty: Type, qs: &[ExistQuant]) -> Type {
    if qs.is_empty() {
        return ty;
    }
    let replace = |name: &str, args: &[Type]| -> Option<Type> {
        if !args.is_empty() {
            return None;
        }
        qs.iter().find(|q| q.name == name).map(|q| {
            if q.lo.is_none() && q.hi.is_none() {
                Type::Wildcard
            } else {
                Type::BoundedWildcard {
                    lo: q.lo.clone().map(Box::new),
                    hi: q.hi.clone().map(Box::new),
                }
            }
        })
    };
    match ty {
        Type::Named { name, args } => {
            if let Some(w) = replace(&name, &args) {
                w
            } else {
                Type::Named {
                    name,
                    args: args.into_iter().map(|a| subst_quantified(a, qs)).collect(),
                }
            }
        }
        Type::Class { sym, args } => Type::Class {
            sym,
            args: args.into_iter().map(|a| subst_quantified(a, qs)).collect(),
        },
        Type::Applied { ctor, args } => Type::Applied {
            ctor: Box::new(subst_quantified(*ctor, qs)),
            args: args.into_iter().map(|a| subst_quantified(a, qs)).collect(),
        },
        Type::Array(t) => Type::Array(Box::new(subst_quantified(*t, qs))),
        Type::Function { params, ret } => Type::Function {
            params: params
                .into_iter()
                .map(|p| subst_quantified(p, qs))
                .collect(),
            ret: Box::new(subst_quantified(*ret, qs)),
        },
        Type::Method { paramss, ret } => Type::Method {
            paramss: paramss
                .into_iter()
                .map(|ps| ps.into_iter().map(|p| subst_quantified(p, qs)).collect())
                .collect(),
            ret: Box::new(subst_quantified(*ret, qs)),
        },
        Type::ByName(t) => Type::ByName(Box::new(subst_quantified(*t, qs))),
        Type::Repeated(t) => Type::Repeated(Box::new(subst_quantified(*t, qs))),
        Type::Tuple(ts) => Type::Tuple(ts.into_iter().map(|t| subst_quantified(t, qs)).collect()),
        Type::Overload(alts) => {
            Type::Overload(alts.into_iter().map(|t| subst_quantified(t, qs)).collect())
        }
        Type::Annotated { tpe, annot } => Type::Annotated {
            tpe: Box::new(subst_quantified(*tpe, qs)),
            annot,
        },
        Type::BoundedWildcard { lo, hi } => Type::BoundedWildcard {
            lo: lo.map(|t| Box::new(subst_quantified(*t, qs))),
            hi: hi.map(|t| Box::new(subst_quantified(*t, qs))),
        },
        other => other,
    }
}

/// Forget what an import prefix was last typed as, down the whole `a.b.c`
/// chain, so the next pass types it from scratch. See `import_prefix`.
pub(crate) fn clear_path_types(t: &mut Tree) {
    match &mut t.kind {
        TreeKind::Select { qual, .. } => {
            t.ty = Type::NoType;
            t.sym = SymbolId::NONE;
            clear_path_types(qual);
        }
        TreeKind::Ident { .. } => {
            t.ty = Type::NoType;
            t.sym = SymbolId::NONE;
        }
        _ => {}
    }
}

pub(crate) fn path_display(t: &Tree) -> String {
    match &t.kind {
        TreeKind::Ident { name } => name.clone(),
        TreeKind::Select { qual, name } => format!("{}.{}", path_display(qual), name),
        TreeKind::SelectFromTypeTree { qual, name, hash } => {
            let op = if *hash { "#" } else { "." };
            format!("{}{op}{name}", path_display(qual))
        }
        TreeKind::This { qual: None } => "this".into(),
        TreeKind::This { qual: Some(q) } => format!("{q}.this"),
        TreeKind::Super { .. } => "super".into(),
        TreeKind::Apply { fun, .. } => format!("{}()", path_display(fun)),
        TreeKind::New { tpt } => format!("new {}", tpt.name().unwrap_or("?")),
        _ => t.name().unwrap_or("<expr>").to_string(),
    }
}

/// `scala.Int` and its siblings, written out as a path.
///
/// The primitives and the top and bottom types are `Type`s of their own here,
/// not class symbols, so resolving `scala.Int` through the package's member
/// list produced a `Type::Class` that *printed* as `Int` and was equal to
/// nothing: `val x: scala.Int = 1` was `type mismatch; found: 1  required:
/// Int`, and `1 + 1` on such a value found no `+` overload. A macro expansion
/// arrives as exactly this path -- `TypeTree(typeOf[Int])` is serialised by
/// full name -- which is how it was found.
pub(crate) fn scala_value_type(qual: &Tree, name: &str) -> Option<Type> {
    let scala_pkg = match &qual.kind {
        TreeKind::Ident { name } => name == "scala",
        TreeKind::Select { qual, name } => {
            name == "scala" && matches!(&qual.kind, TreeKind::Ident { name } if name == "_root_")
        }
        _ => false,
    };
    if !scala_pkg {
        return None;
    }
    Some(match name {
        "Boolean" => Type::Boolean,
        "Byte" => Type::Byte,
        "Short" => Type::Short,
        "Char" => Type::Char,
        "Int" => Type::Int,
        "Long" => Type::Long,
        "Float" => Type::Float,
        "Double" => Type::Double,
        "Unit" => Type::Unit,
        "Any" => Type::Any,
        "AnyVal" => Type::AnyVal,
        "AnyRef" => Type::AnyRef,
        "Nothing" => Type::Nothing,
        "Null" => Type::Null,
        _ => return None,
    })
}

pub(crate) fn structural_select_lhs(lhs: &Tree) -> bool {
    match &lhs.kind {
        TreeKind::Select { qual, .. } => match &qual.ty {
            Type::Refined { decls, .. } => SymbolTable::refined_has_term_members(decls),
            _ => false,
        },
        _ => false,
    }
}

pub fn has_errors(diags: &[Diagnostic]) -> bool {
    diags.iter().any(|d| d.level == scala_rs_span::Level::Error)
}

/// A function literal whose parameters all carry a type annotation
/// (`(x: String) => x.length`). Its type is known without an expected type,
/// so it can be typed eagerly and drive type-parameter inference.
pub(crate) fn is_annotated_lambda(tree: &Tree) -> bool {
    match &tree.kind {
        TreeKind::Function { vparams, .. } => {
            !vparams.is_empty()
                && vparams.iter().all(|p| match &p.kind {
                    TreeKind::ValDef { tpt, .. } => !tpt.is_empty(),
                    _ => false,
                })
        }
        _ => false,
    }
}

/// The written name an application ultimately calls, past any type
/// application: the node a `reify` body's classification has to be keyed on,
/// since that is the one `crate::reify` asks about.
pub(crate) fn reify_callee(fun: &Tree) -> &Tree {
    let mut head = fun;
    while let TreeKind::TypeApply { fun, .. } = &head.kind {
        head = fun;
    }
    head
}

/// Drop diagnostics repeated verbatim at the same position, keeping the first.
fn dedup_diags(diags: &mut Vec<Diagnostic>) {
    let mut seen = std::collections::HashSet::new();
    diags.retain(|d| {
        seen.insert((
            d.file_index,
            d.span.lo,
            d.span.hi,
            d.level,
            d.message.clone(),
        ))
    });
}

/// Re-read a parent's `this.type` as the overriding class's own.
///
/// `trait Nd { type Self >: this.type <: Nd }` overridden by
/// `class Leafy extends Nd { type Self = Leafy }` has to compare `Leafy`
/// against `Leafy.this.type`, not against `Nd.this.type`.
pub(crate) fn retarget_this(ty: &Type, cls: SymbolId) -> Type {
    match ty {
        Type::ThisType(_) => Type::ThisType(cls),
        Type::Class { sym, args } => Type::Class {
            sym: *sym,
            args: args.iter().map(|a| retarget_this(a, cls)).collect(),
        },
        Type::Refined { parents, decls } => Type::Refined {
            parents: parents.iter().map(|p| retarget_this(p, cls)).collect(),
            decls: decls.clone(),
        },
        _ => ty.clone(),
    }
}

/// Number of parameters a `FunctionN` expected type takes, if it is one.
pub(crate) fn expected_function_arity(pt: &Type) -> Option<usize> {
    match pt {
        Type::Function { params, .. } => Some(params.len()),
        Type::Named { name, args } if name.starts_with("Function") && name != "Function" => {
            (!args.is_empty()).then(|| args.len() - 1)
        }
        _ => None,
    }
}
