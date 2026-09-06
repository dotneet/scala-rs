//! Walk a typed compilation unit and emit JVM classfiles (major 52).

use crate::classfile::{
    encode_method_name, ClassEmit, EmittedClass, Field, InnerClassEntry, Method, Pool, ACC_FINAL,
    ACC_PRIVATE, ACC_PROTECTED, ACC_PUBLIC, ACC_STATIC, ACC_SUPER, MAX_CODE_LENGTH,
};
use crate::code::{Assembler, Label};
use crate::companion_fwd::{self};
use crate::ifacebridge::BinaryParents;
use scala_rs_parser::{Flags, SymbolId, Tree, TreeKind, Type};
use scala_rs_typer::{SymKind, SymbolTable};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

// Re-exported so every `gen_*` sibling sees the whole module.
pub(crate) use crate::gen_call::*;
pub(crate) use crate::gen_desc::*;
pub(crate) use crate::gen_expr::*;
pub(crate) use crate::gen_invoke::*;
pub use crate::gen_lambda::collect_captured_vars;
pub(crate) use crate::gen_lambda::*;
pub(crate) use crate::gen_match::*;
pub(crate) use crate::gen_object::*;
use scala_rs_span::Span;

/// Options for [`emit_opts`].
#[derive(Clone, Debug, Default)]
pub struct EmitOpts {
    /// Emit invokes against scala-library 2.13 (Option/List/`::`/FunctionN/…).
    ///
    /// The private runtime classfiles are not part of this function; the driver
    /// skips them when this flag is set. Call sites that would miss on the jar
    /// (`List.withFilter`, `List.tail()List`, ArrowAssoc) are rewritten here.
    pub library_abi: bool,
    /// Pre-erasure ScalaSignature pickles, keyed by class symbol id.
    ///
    /// Shared, not owned: the driver hands the same map to every unit of the
    /// run, and deep-copying it per unit was 3% of the compile.
    pub pickles: Rc<HashMap<u32, Vec<u8>>>,
    /// Concrete trait members from every unit in the run; `None` means this
    /// unit only. Shared for the same reason (9% of the compile).
    pub trait_members: Option<Rc<TraitImpls>>,
    /// JVM internal name -> class-like symbol, for the whole table. A pure
    /// function of `st`, which does not change while a run is emitted, so the
    /// driver builds it once; `None` builds it here.
    pub jvm_index: Option<Rc<HashMap<String, SymbolId>>>,
    /// Mutable locals captured by a class defined inside a method, for the
    /// whole table. Also a pure function of `st`, and finding them means
    /// reading every symbol's `captures`: doing that once per unit was another
    /// files-times-symbols sweep. `None` computes it here.
    pub captured_vars: Option<Rc<HashSet<SymbolId>>>,
    /// Simple name -> the first non-trait class symbol carrying it, for
    /// [`Gen::find_class_named`]. A pure function of `st` as well; the search
    /// it replaces was a linear scan of every symbol, run once per module
    /// emitted. `None` builds it here.
    pub class_by_name: Option<Rc<HashMap<String, SymbolId>>>,
    /// Class files of the run's `-cp` / `--scala-library`, for the bridges a
    /// class needs against members it only inherits (see [`crate::ifacebridge`]).
    /// `None` skips that pass, which is what the private-runtime ABI wants.
    pub binary_parents: Option<Rc<BinaryParents>>,
    /// JVMS §4.7.9 `Signature` candidates, read from the symbol table before
    /// erasure destroyed the types they describe (see [`crate::sig`]). `None`
    /// emits no `Signature` attribute at all, which is what a caller that
    /// never ran [`crate::sig::record_generic_signatures`] must get.
    pub generic_sigs: Option<Rc<crate::sig::GenericSignatures>>,
}

/// A backend limitation discovered while lowering a typed tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmitError {
    pub span: Span,
    pub message: String,
}

/// Result of emitting one compilation unit.
pub type EmitResult = Result<Vec<EmittedClass>, Vec<EmitError>>;

/// Walk a typed compilation unit and emit classes (private-runtime ABI).
pub fn emit(tree: &Tree, st: &SymbolTable, source_name: &str) -> EmitResult {
    emit_opts(tree, st, source_name, EmitOpts::default())
}

/// Concrete trait members of a whole run, so a class can mix in a trait
/// defined in another file.
#[derive(Default, Clone, Debug)]
pub struct TraitImpls {
    pub(crate) impls: HashMap<SymbolId, Vec<Tree>>,
    pub(crate) vals: HashMap<SymbolId, Vec<Tree>>,
    /// `vals` plus the trait body's bare expression statements, in source
    /// order — the whole of what `$init$` runs (SLS 5.1).
    pub(crate) inits: HashMap<SymbolId, Vec<Tree>>,
    pub(crate) lazy_vals: HashMap<SymbolId, Vec<Tree>>,
    pub(crate) modules: HashMap<SymbolId, Vec<Tree>>,
}

/// Collect the concrete trait members of one unit into a shared map.
///
/// `st` is unused — the harvest reads the tree only — and is kept so callers
/// need not change.
pub fn collect_trait_members(tree: &Tree, _st: &SymbolTable, into: &mut TraitImpls) {
    collect_trait_impls(tree, into);
}

/// Record on the symbol which trait members write `super.m` in their body,
/// so the pickler can declare nsc's `SUPERACCESSOR` member for each.
///
/// nsc's mixin phase implements `p$q$T$$super$m` in a class **only** when the
/// trait's signature declares a member carrying that flag. We emit the
/// accessor on the interface either way, so a stackable `abstract override`
/// trait of ours mixed in by real scalac used to run the *base* implementation
/// and silently drop the trait's own layer.
///
/// Deliberately narrower than [`needs_super_accessor`], which also declares an
/// accessor for a plain `override def`: nothing calls those, and asking a
/// reader to implement `T$$super$m` where the overridden member is itself
/// deferred has no target to forward to.
pub fn mark_super_accessors(tree: &Tree, st: &mut SymbolTable) {
    let mut found = Vec::new();
    collect_super_accessors(tree, &mut found);
    for id in found {
        st.get_mut(id).super_accessor = true;
    }
}

pub(crate) fn collect_super_accessors(tree: &Tree, out: &mut Vec<SymbolId>) {
    if let TreeKind::ClassDef { mods, impl_, .. } = &tree.kind {
        if mods.flags.contains(Flags::TRAIT) {
            for stt in &impl_.body {
                if let TreeKind::DefDef { rhs, .. } = &stt.kind {
                    if !stt.sym.is_none() && needs_super_accessor(stt) && tree_contains_super(rhs) {
                        out.push(stt.sym);
                    }
                }
            }
        }
    }
    for_each_term_child(tree, &mut |c| collect_super_accessors(c, out));
}

/// Harvest one unit's concrete trait members. A function of the tree alone:
/// no symbol table, no ABI, so running it twice on the same tree inserts the
/// same entries under the same keys.
pub(crate) fn collect_trait_impls(tree: &Tree, into: &mut TraitImpls) {
    match &tree.kind {
        TreeKind::ClassDef { mods, impl_, .. } => {
            if mods.flags.contains(Flags::TRAIT) {
                let methods: Vec<Tree> = impl_
                    .body
                    .iter()
                    .filter(|s| match &s.kind {
                        TreeKind::DefDef { rhs, name, .. } => {
                            !rhs.is_empty() && name != "<init>" && name != "<clinit>"
                        }
                        _ => false,
                    })
                    .cloned()
                    .collect();
                if !methods.is_empty() && !tree.sym.is_none() {
                    into.impls.insert(tree.sym, methods);
                }
                let vals: Vec<Tree> = impl_
                    .body
                    .iter()
                    .filter(|s| match &s.kind {
                        TreeKind::ValDef { rhs, mods, .. } => {
                            !rhs.is_empty() && !mods.flags.contains(Flags::LAZY)
                        }
                        _ => false,
                    })
                    .cloned()
                    .collect();
                if !vals.is_empty() && !tree.sym.is_none() {
                    into.vals.insert(tree.sym, vals);
                }
                // `$init$` runs the `val` initializers *and* the trait
                // body's bare statements, in source order (SLS 5.1).
                let init_stats: Vec<Tree> = impl_
                    .body
                    .iter()
                    .filter(|s| match &s.kind {
                        TreeKind::ValDef { rhs, mods, .. } => {
                            !rhs.is_empty() && !mods.flags.contains(Flags::LAZY)
                        }
                        _ => is_template_stat(s),
                    })
                    .cloned()
                    .collect();
                if !init_stats.is_empty() && !tree.sym.is_none() {
                    into.inits.insert(tree.sym, init_stats);
                }
                let lazies: Vec<Tree> = impl_
                    .body
                    .iter()
                    .filter(|s| match &s.kind {
                        TreeKind::ValDef { rhs, mods, .. } => {
                            !rhs.is_empty() && mods.flags.contains(Flags::LAZY)
                        }
                        _ => false,
                    })
                    .cloned()
                    .collect();
                if !lazies.is_empty() && !tree.sym.is_none() {
                    into.lazy_vals.insert(tree.sym, lazies);
                }
                // A `case class` declared in a trait carries a *synthesized*
                // companion, which is a member `object` of that trait exactly
                // as a written one is: the trait declares an abstract `K()`
                // accessor and every class mixing it in owes an
                // implementation. Only the `ModuleDef`s were harvested, so
                // `trait T { case class K(a: Int) }; object P extends T` threw
                // `AbstractMethodError: P$ … K()` at the first `P.K(1)`.
                // The companion is resolved in `mixin_member_modules`, which
                // has the symbol table; this pass reads the tree alone.
                let modules: Vec<Tree> = impl_
                    .body
                    .iter()
                    .filter(|s| match &s.kind {
                        TreeKind::ModuleDef { .. } => true,
                        TreeKind::ClassDef { mods, .. } => mods.flags.contains(Flags::CASE),
                        _ => false,
                    })
                    .cloned()
                    .collect();
                if !modules.is_empty() && !tree.sym.is_none() {
                    into.modules.insert(tree.sym, modules);
                }
            }
            for_each_term_child(tree, &mut |c| collect_trait_impls(c, into));
        }
        // A `trait` is not only a template member: it can be declared in
        // any block — a method body, an `if` branch, a lambda. Those local
        // traits need their concrete members harvested exactly like a
        // top-level one's, or every class mixing them in is emitted with
        // no mixin forwarders at all and fails at run time with
        // `AbstractMethodError`.
        _ => for_each_term_child(tree, &mut |c| collect_trait_impls(c, into)),
    }
}

/// Walk a typed compilation unit and emit classes.
pub fn emit_opts(tree: &Tree, st: &SymbolTable, source_name: &str, opts: EmitOpts) -> EmitResult {
    // A shared map already holds this unit's own trait members: the driver
    // harvests every unit of the run before emitting any, and the harvest is a
    // function of the tree alone, so doing it again here would insert the same
    // entries under the same keys. Only the `None` caller has to do its own.
    let traits = match opts.trait_members {
        Some(shared) => shared,
        None => {
            let mut own = TraitImpls::default();
            collect_trait_impls(tree, &mut own);
            Rc::new(own)
        }
    };
    let mut g = Gen {
        st,
        source_name,
        unit_span: tree.span,
        out: Vec::new(),
        emit_errors: Rc::new(RefCell::new(Vec::new())),
        extras: RefCell::new(Vec::new()),
        lambda_n: Cell::new(0),
        traits,
        lambda_bodies: RefCell::new(Vec::new()),
        library_abi: opts.library_abi,
        pickles: opts.pickles,
        // `scala.runtime.*Ref` exists in both ABIs: on the jar, and as a
        // private-runtime classfile (see `runtime::REF_BOXES`).
        boxed_vars: collect_boxed_vars(tree, st, opts.captured_vars.as_ref()),
        jvm_index: opts
            .jvm_index
            .unwrap_or_else(|| Rc::new(build_jvm_index(st))),
        class_by_name: opts
            .class_by_name
            .unwrap_or_else(|| Rc::new(build_class_name_index(st))),
        binary_parents: opts.binary_parents,
        generic_sigs: opts.generic_sigs.unwrap_or_default(),
        companion_fwd: HashMap::new(),
        parked_companions: Vec::new(),
    };
    g.walk(tree);
    g.flush_parked_companions();
    g.emit_anon_classes(tree);
    debug_assert!(
        g.lambda_bodies.borrow().is_empty(),
        "a hoisted lambda body was queued but never written to a classfile"
    );
    g.out.append(&mut g.extras.borrow_mut());
    let errors = g.emit_errors.borrow().clone();
    if errors.is_empty() {
        Ok(g.out)
    } else {
        Err(errors)
    }
}

pub(crate) struct Gen<'a> {
    pub(crate) st: &'a SymbolTable,
    pub(crate) source_name: &'a str,
    pub(crate) unit_span: Span,
    pub(crate) out: Vec<EmittedClass>,
    pub(crate) emit_errors: Rc<RefCell<Vec<EmitError>>>,
    pub(crate) extras: RefCell<Vec<EmittedClass>>,
    pub(crate) lambda_n: Cell<u32>,
    /// Lambda bodies hoisted out of closures, waiting to be written as
    /// static methods of the classfile currently under construction. A
    /// nested class emitted mid-flight records its own watermark, so it
    /// drains only what it queued (see [`Gen::drain_lambdas`]).
    pub(crate) lambda_bodies: RefCell<Vec<PendingBody>>,
    /// Concrete trait methods, for the interface's `default` bodies and the
    /// mixin forwarders each implementing class carries.
    pub(crate) traits: Rc<TraitImpls>,
    /// Trait `val` definitions with a right-hand side (`<Iface>.$init$`).
    /// What `<Iface>.$init$` actually runs: the entries of `trait_vals`
    /// interleaved with the trait body's bare expression statements, in source
    /// order (SLS 5.1). Kept apart from `trait_vals` because the accessor /
    /// mixin-forwarder passes want the `val`s alone.
    /// Trait `lazy val` definitions. Unlike a plain `val` these are not set
    /// from `$init$`; every implementing class gets its own field, bitmap bit
    /// and accessor, exactly as nsc's mixin phase does.
    /// Member `object`s declared in a trait. Like a trait `lazy val` these are
    /// not set from `$init$`: every implementing class gets its own
    /// `<name>$module` field and `<name>()` accessor, as nsc's mixin phase does.
    pub(crate) library_abi: bool,
    pub(crate) pickles: Rc<HashMap<u32, Vec<u8>>>,
    /// Locals boxed into `scala.runtime.IntRef` / `ObjectRef` (library ABI).
    pub(crate) boxed_vars: HashSet<SymbolId>,
    /// JVM internal name → class-like symbol, for the whole symbol table.
    /// Built once; used to compute `InnerClasses`/`EnclosingMethod`.
    pub(crate) jvm_index: Rc<HashMap<String, SymbolId>>,
    /// Simple name → first non-trait class symbol with it, for the
    /// case-companion lookup in `emit_module`. Built once; that lookup used to
    /// scan every symbol, once per module in the run.
    pub(crate) class_by_name: Rc<HashMap<String, SymbolId>>,
    /// Class files behind the run's binary parents (see
    /// [`Gen::emit_binary_parent_bridges`]).
    pub(crate) binary_parents: Option<Rc<BinaryParents>>,
    /// Generic signatures taken before erasure, keyed by symbol. Empty when
    /// the driver did not record any.
    pub(crate) generic_sigs: Rc<crate::sig::GenericSignatures>,
    /// Static forwarders a top-level `object` owes its companion class,
    /// keyed by that class's JVM internal name. Filled by [`Gen::emit_module`]
    /// and drained by [`Gen::finish_companion_class`] — the two run in source
    /// order, so either one can come first.
    pub(crate) companion_fwd: HashMap<String, Vec<companion_fwd::Forwarder>>,
    /// Companion classes whose builder is complete but whose `object` has not
    /// been emitted yet, so its forwarders are still unknown. A `Vec` rather
    /// than a map to keep the emission order of everything else fixed.
    pub(crate) parked_companions: Vec<(String, ClassBuilder)>,
}

/// JVM internal name → class-like symbol, for every `Class`/`ModuleClass` in
/// `st` (the current unit plus everything installed from the classpath).
/// Built once per [`Gen`] and consulted by [`ClassBuilder::finish_full`] to
/// compute `InnerClasses` entries without a linear scan per classfile.
pub fn build_jvm_index(st: &SymbolTable) -> HashMap<String, SymbolId> {
    let mut m = HashMap::new();
    for s in &st.symbols {
        if matches!(s.kind, SymKind::Class | SymKind::ModuleClass) {
            m.entry(st.jvm_internal(s.id)).or_insert(s.id);
        }
    }
    m
}

/// Simple name → the first non-trait class symbol carrying it, in symbol
/// order. This is what a linear `find` over `st.symbols` used to answer for
/// every module emitted, so it is built once for the run instead.
pub fn build_class_name_index(st: &SymbolTable) -> HashMap<String, SymbolId> {
    let mut m = HashMap::new();
    for s in &st.symbols {
        if s.kind == SymKind::Class && !s.flags.contains(Flags::TRAIT) {
            m.entry(s.name.clone()).or_insert(s.id);
        }
    }
    m
}

/// A lambda body hoisted out of an anonymous class and into a `private
/// static` method of the class that lexically contains it — the shape nsc
/// 2.13 emits, and the one `LambdaMetafactory` links an `invokedynamic` call
/// site to. Queued while the enclosing method is being assembled (its
/// `ClassBuilder` is borrowed by the `Assembler` at that moment) and drained
/// by [`drain_lambda_bodies`] once the class's own methods are done.
pub(crate) struct PendingBody {
    /// Method name on the owning classfile, e.g. `$anonfun$7`.
    pub(crate) name: String,
    /// `(<outer?><captures…><args…>)Ljava/lang/Object;`
    pub(crate) desc: String,
    /// Whether parameter 0 is the enclosing instance.
    pub(crate) has_outer: bool,
    /// The enclosing class as the body sees it (`class_name` of the call
    /// site, which for a trait's bodies is the *interface* itself).
    pub(crate) outer_class: String,
    /// Symbol of the class the body is lexically inside.
    pub(crate) class_sym: SymbolId,
    pub(crate) vparams: Vec<Tree>,
    pub(crate) body: Tree,
    pub(crate) local_caps: Vec<SymbolId>,
    /// Result type of the lambda, for the boxing the epilogue does.
    pub(crate) ret_ty: Type,
}

pub(crate) struct EmitCtx<'a> {
    pub(crate) st: &'a SymbolTable,
    pub(crate) class_sym: SymbolId,
    pub(crate) class_name: &'a str,
    pub(crate) ret_ty: Type,
    pub(crate) emit_errors: Rc<RefCell<Vec<EmitError>>>,
    pub(crate) extras: &'a RefCell<Vec<EmittedClass>>,
    pub(crate) lambda_n: &'a Cell<u32>,
    /// Lambda bodies waiting to become static methods of `hoist_owner`.
    pub(crate) lambda_bodies: &'a RefCell<Vec<PendingBody>>,
    /// Internal name of the classfile currently being built, when it can
    /// take extra static methods. An interface counts: nsc puts a trait's
    /// `$anonfun$` bodies on the interface too (only `ACC_FINAL` has to come
    /// off — see [`emit_lambda_body`]). `None` makes every lambda in that
    /// context fall back to an anonymous class.
    pub(crate) hoist_owner: Option<&'a str>,
    pub(crate) source: &'a str,
    /// If generating inside a lambda, field on the lambda class holding the outer `this`.
    pub(crate) outer: Option<(&'a str, &'a str, &'a str)>, // (lambda_class, field, outer_desc)
    /// Set while emitting the part of `<init>` that runs *before* the super
    /// constructor call (super-constructor arguments, early definitions).
    /// `this` is still `uninitializedThis` there, and JVMS §4.10.1.9 lets
    /// `putfield` take that but not `getfield`: reading `this.$outer` before
    /// the super call is a `VerifyError` even after the field was stored.
    /// The enclosing instance is in this local slot instead — the `<init>`
    /// parameter it arrived in, which is what nsc reads there too.
    /// `(slot, class we step into, static type on the stack)`.
    pub(crate) presuper_outer: Option<(u16, SymbolId, SymbolId)>,
    /// Inside a hoisted lambda body the enclosing instance is an ordinary
    /// parameter of a *static* method, not a field of a closure object:
    /// `load_this` reads this local slot instead of `this.$outer`.
    pub(crate) outer_slot: Option<u16>,
    pub(crate) library_abi: bool,
    /// Named JVM method being emitted; `NONE` inside lambdas.
    pub(crate) method_sym: SymbolId,
    /// Captured `var`s lowered to `scala.runtime.*Ref`.
    pub(crate) boxed_vars: &'a HashSet<SymbolId>,
    /// Set while emitting a value class's `$extension` static: there is no
    /// `this` there, only the underlying value in slot 0, so anything that
    /// really needs the boxed instance has to build one.
    /// `(class internal name, `<init>` descriptor, underlying slot sort)`.
    pub(crate) value_ext: Option<(String, String, JvmSort)>,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_ctx<'a>(
    st: &'a SymbolTable,
    class_sym: SymbolId,
    class_name: &'a str,
    ret_ty: Type,
    extras: &'a RefCell<Vec<EmittedClass>>,
    lambda_n: &'a Cell<u32>,
    lambda_bodies: &'a RefCell<Vec<PendingBody>>,
    hoist_owner: Option<&'a str>,
    source: &'a str,
    library_abi: bool,
    boxed_vars: &'a HashSet<SymbolId>,
    emit_errors: Rc<RefCell<Vec<EmitError>>>,
) -> EmitCtx<'a> {
    EmitCtx {
        st,
        class_sym,
        class_name,
        ret_ty,
        emit_errors,
        extras,
        lambda_n,
        lambda_bodies,
        hoist_owner,
        source,
        outer: None,
        presuper_outer: None,
        outer_slot: None,
        library_abi,
        method_sym: SymbolId::NONE,
        boxed_vars,
        value_ext: None,
    }
}

pub(crate) fn report_emit_error(
    errors: &Rc<RefCell<Vec<EmitError>>>,
    span: Span,
    message: impl Into<String>,
) {
    errors.borrow_mut().push(EmitError {
        span,
        message: message.into(),
    });
}

pub(crate) fn report_ctx_error(ctx: &EmitCtx<'_>, span: Span, message: impl Into<String>) {
    report_emit_error(&ctx.emit_errors, span, message);
}

/// The `presuper_outer` an `<init>` of `class_id` needs: the enclosing
/// instance arrives in local slot 1, the walk steps into `class_id`'s
/// enclosing class, and the value on the stack has the `$outer` field's type.
pub(crate) fn presuper_outer_of(
    st: &SymbolTable,
    class_id: SymbolId,
) -> Option<(u16, SymbolId, SymbolId)> {
    let next = enclosing_instance(st, class_id)?;
    let held = outer_field_class(st, class_id).unwrap_or(next);
    Some((1, next, held))
}

/// Push the value an `$outer` chain walk starts from, and return
/// `(class we are lexically inside, static type on the stack)`.
///
/// Normally that is `this` in the class being emitted. `needs_hop` says the
/// caller is about to step out at least once; in the pre-super part of a
/// nested class's `<init>` that first hop cannot be a `getfield` (see
/// `EmitCtx::presuper_outer`), so it is the constructor's own `$outer`
/// argument instead.
pub(crate) fn start_outer_walk(
    asm: &mut Assembler,
    ctx: &EmitCtx,
    needs_hop: bool,
) -> (SymbolId, SymbolId) {
    if needs_hop {
        if let Some((slot, next, held)) = ctx.presuper_outer {
            asm.aload(slot);
            return (next, held);
        }
    }
    load_this(asm, ctx);
    (ctx.class_sym, ctx.class_sym)
}

pub(crate) fn runtime_ref_class(ty: &Type) -> &'static str {
    match ty.widen_constant() {
        Type::Int => "scala/runtime/IntRef",
        Type::Long => "scala/runtime/LongRef",
        Type::Double => "scala/runtime/DoubleRef",
        Type::Float => "scala/runtime/FloatRef",
        Type::Boolean => "scala/runtime/BooleanRef",
        Type::Byte => "scala/runtime/ByteRef",
        Type::Short => "scala/runtime/ShortRef",
        Type::Char => "scala/runtime/CharRef",
        _ => "scala/runtime/ObjectRef",
    }
}

pub(crate) fn runtime_ref_elem_desc(ty: &Type) -> &'static str {
    match ty.widen_constant() {
        Type::Int => "I",
        Type::Long => "J",
        Type::Double => "D",
        Type::Float => "F",
        Type::Boolean => "Z",
        Type::Byte => "B",
        Type::Short => "S",
        Type::Char => "C",
        _ => "Ljava/lang/Object;",
    }
}

pub(crate) fn runtime_ref_create_desc(ty: &Type) -> String {
    format!("({})L{};", runtime_ref_elem_desc(ty), runtime_ref_class(ty))
}

pub(crate) fn is_boxed_var(ctx: &EmitCtx, id: SymbolId) -> bool {
    !id.is_none() && ctx.boxed_vars.contains(&id)
}

pub(crate) fn emit_runtime_ref_create(asm: &mut Assembler, ty: &Type) {
    let cls = runtime_ref_class(ty);
    asm.invokestatic(cls, "create", &runtime_ref_create_desc(ty));
}

pub(crate) fn load_runtime_ref_elem(asm: &mut Assembler, ctx: &EmitCtx, ty: &Type) {
    let cls = runtime_ref_class(ty);
    let elem = runtime_ref_elem_desc(ty);
    asm.getfield(cls, "elem", elem);
    if elem == "Ljava/lang/Object;" {
        if matches!(ty, Type::String) {
            asm.checkcast("java/lang/String");
        } else if let Some(cn) = checkcast_internal(ctx.st, ty) {
            if cn != "java/lang/Object" {
                asm.checkcast(&cn);
            }
        }
    }
}

pub(crate) fn store_runtime_ref_elem(asm: &mut Assembler, ty: &Type) {
    let cls = runtime_ref_class(ty);
    let elem = runtime_ref_elem_desc(ty);
    if elem == "Ljava/lang/Object;" && is_jvm_primitive(ty) && !is_unit_like(ty) {
        emit_box(asm, ty);
    }
    asm.putfield(cls, "elem", elem);
}

pub(crate) fn jvm_desc_maybe_boxed(
    st: &SymbolTable,
    ty: &Type,
    id: SymbolId,
    boxed: &HashSet<SymbolId>,
) -> String {
    if !id.is_none() && boxed.contains(&id) {
        format!("L{};", runtime_ref_class(ty))
    } else {
        jvm_desc_val(st, ty)
    }
}

// ---------------------------------------------------------------------------
// captured enclosing-method locals (`new T { … }`, local `class`)
// ---------------------------------------------------------------------------

/// Enclosing-method locals a class defined inside a method has to receive.
/// Filled by the typer's `anon_capture` pass; empty for every other class.
pub(crate) fn class_captures(st: &SymbolTable, class_id: SymbolId) -> &[SymbolId] {
    if class_id.is_none() {
        return &[];
    }
    &st.get(class_id).captures
}

/// Field / constructor-parameter name of the `idx`-th capture (nsc: `x$1`).
pub(crate) fn capture_field_name(st: &SymbolTable, id: SymbolId, idx: usize) -> String {
    format!("{}${}", st.get(id).name, idx + 1)
}

/// Descriptor of a captured value; a `scala.runtime.*Ref` for captured `var`s.
pub(crate) fn capture_field_desc(
    st: &SymbolTable,
    boxed: &HashSet<SymbolId>,
    id: SymbolId,
) -> String {
    jvm_desc_maybe_boxed(st, &st.get(id).ty, id, boxed)
}

pub(crate) fn capture_field_sort(
    boxed: &HashSet<SymbolId>,
    st: &SymbolTable,
    id: SymbolId,
) -> JvmSort {
    if boxed.contains(&id) {
        JvmSort::Ref
    } else {
        jvm_sort(&st.get(id).ty)
    }
}

/// The capture constructor parameters of `class_id`, as descriptor text.
pub(crate) fn capture_params_desc(
    st: &SymbolTable,
    boxed: &HashSet<SymbolId>,
    class_id: SymbolId,
) -> String {
    class_captures(st, class_id)
        .iter()
        .map(|c| capture_field_desc(st, boxed, *c))
        .collect()
}

/// Splice extra parameter descriptors in front of the `)` of `desc`.
pub(crate) fn desc_with_extra_params(desc: &str, extra: &str) -> String {
    if extra.is_empty() {
        return desc.to_string();
    }
    match desc.rfind(')') {
        Some(i) => format!("{}{}{}", &desc[..i], extra, &desc[i..]),
        None => desc.to_string(),
    }
}

/// `(symbol, field name, field descriptor, JVM sort)` per capture.
pub(crate) type CaptureSlots = Vec<(SymbolId, String, String, JvmSort)>;

pub(crate) fn capture_slots(
    st: &SymbolTable,
    boxed: &HashSet<SymbolId>,
    class_id: SymbolId,
) -> CaptureSlots {
    class_captures(st, class_id)
        .iter()
        .enumerate()
        .map(|(i, c)| {
            (
                *c,
                capture_field_name(st, *c, i),
                capture_field_desc(st, boxed, *c),
                capture_field_sort(boxed, st, *c),
            )
        })
        .collect()
}

/// A `trait` has no constructor, so an enclosing-method local its body reads
/// cannot be a constructor parameter the way a local `class`'s is. The only
/// handle the trait's body has is the receiver in slot 0, typed as the
/// interface — so each captured local becomes an accessor the interface
/// declares abstract and every implementing class provides from its own
/// capture field (nsc does the same, as `outerVal$1()` plus a mixin setter).
///
/// Keyed by the captured symbol rather than by its position in any one
/// trait's capture list: two traits mixed into the same class may capture
/// different locals that share a simple name, and a positional name would
/// then collapse them into one accessor.
pub(crate) fn capture_accessor_name(st: &SymbolTable, id: SymbolId) -> String {
    format!("{}${}", st.get(id).name, id.0)
}

/// `(symbol, accessor name, getter descriptor, sort)` for each enclosing-method
/// local `trait_id` itself captures. Empty for every non-local trait.
pub(crate) fn trait_capture_accessors(
    st: &SymbolTable,
    boxed: &HashSet<SymbolId>,
    trait_id: SymbolId,
) -> Vec<(SymbolId, String, String, JvmSort)> {
    class_captures(st, trait_id)
        .iter()
        .map(|c| {
            (
                *c,
                capture_accessor_name(st, *c),
                format!("(){}", capture_field_desc(st, boxed, *c)),
                capture_field_sort(boxed, st, *c),
            )
        })
        .collect()
}

/// Read the captured locals of a trait back through the interface accessors,
/// so the ordinary `Ident` path finds them in the frame while emitting the
/// trait's own bodies. Mirrors [`emit_capture_prologue`], but the values
/// come from `$this` rather than from `this`'s own fields.
pub(crate) fn emit_trait_capture_prologue(
    asm: &mut Assembler,
    frame: &mut Frame,
    iface: &str,
    caps: &[(SymbolId, String, String, JvmSort)],
) {
    for (id, aname, adesc, sort) in caps {
        asm.aload(0);
        asm.invokeinterface(iface, aname, adesc);
        let slot = frame.alloc(*id, *sort);
        store(asm, slot, *sort);
    }
}

/// Read the capture fields into fresh locals at method entry, so the ordinary
/// `Ident` path keeps finding the enclosing-method symbols in the frame.
pub(crate) fn emit_capture_prologue(
    asm: &mut Assembler,
    frame: &mut Frame,
    class_name: &str,
    caps: &CaptureSlots,
) {
    for (id, fname, fdesc, sort) in caps {
        asm.aload(0);
        asm.getfield(class_name, fname, fdesc);
        let slot = frame.alloc(*id, *sort);
        store(asm, slot, *sort);
    }
}

/// Push the current value of a captured local for a `new` of a capturing class.
pub(crate) fn load_capture_arg(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    id: SymbolId,
    span: Span,
) {
    if let Some((slot, sort)) = frame.get(id) {
        if is_boxed_var(ctx, id) {
            // Forward the IntRef/ObjectRef itself, not its `elem`.
            load(asm, slot, JvmSort::Ref);
        } else {
            load(asm, slot, sort);
        }
        return;
    }
    // Not a local here: we are inside a class that captured it as well.
    let own = class_captures(ctx.st, ctx.class_sym);
    if let Some(i) = own.iter().position(|c| *c == id) {
        load_this(asm, ctx);
        asm.getfield(
            &class_internal(ctx.st, ctx.class_sym),
            &capture_field_name(ctx.st, id, i),
            &capture_field_desc(ctx.st, ctx.boxed_vars, id),
        );
        return;
    }
    report_ctx_error(ctx, span, format!("cannot capture {}", ctx.st.get(id).name));
    throw_runtime(asm, &format!("cannot capture {}", ctx.st.get(id).name));
    asm.aconst_null();
}

pub(crate) fn def_is_synthetic(st: &SymbolTable, def: &Tree) -> bool {
    if !def.sym.is_none() && st.get(def.sym).flags.contains(Flags::SYNTHETIC) {
        return true;
    }
    if let TreeKind::DefDef { mods, .. } = &def.kind {
        return mods.flags.contains(Flags::SYNTHETIC);
    }
    false
}

pub(crate) fn def_method_desc_boxed(
    st: &SymbolTable,
    def: &Tree,
    boxed: &HashSet<SymbolId>,
) -> String {
    let synthetic = def_is_synthetic(st, def);
    let mut s = String::from("(");
    if let TreeKind::DefDef { vparamss, .. } = &def.kind {
        for p in vparamss.iter().flatten() {
            let ty = if !p.ty.is_no_type() && !p.ty.is_error() {
                p.ty.clone()
            } else if !p.sym.is_none() {
                st.get(p.sym).ty.clone()
            } else {
                Type::Any
            };
            if synthetic {
                s.push_str(&jvm_desc_maybe_boxed(st, &ty, p.sym, boxed));
            } else {
                s.push_str(&jvm_desc_val(st, &ty));
            }
        }
    }
    s.push(')');
    s.push_str(&jvm_desc(st, &method_ret_ty(def)));
    s
}

pub(crate) fn method_desc_boxed(
    st: &SymbolTable,
    id: SymbolId,
    boxed: &HashSet<SymbolId>,
) -> String {
    let s = st.get(id);
    if s.name == "<init>" || s.jvm_name.starts_with('(') {
        return method_desc_from_sym(st, id);
    }
    let synthetic = s.flags.contains(Flags::SYNTHETIC);
    let params: Vec<Type> = method_params_from_sym(st, id);
    let ret = method_ret_from_sym(st, id);
    let mut d = String::from("(");
    for (i, p) in params.iter().enumerate() {
        let pid = s.params.get(i).copied().unwrap_or(SymbolId::NONE);
        if synthetic {
            d.push_str(&jvm_desc_maybe_boxed(st, p, pid, boxed));
        } else {
            d.push_str(&jvm_desc_val(st, p));
        }
    }
    d.push(')');
    d.push_str(&jvm_desc(st, &ret));
    d
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JvmSort {
    Int,
    Long,
    Float,
    Double,
    Ref,
    Void,
}

impl JvmSort {
    pub(crate) fn slots(self) -> u16 {
        match self {
            JvmSort::Long | JvmSort::Double => 2,
            JvmSort::Void => 0,
            _ => 1,
        }
    }
}

pub(crate) struct Frame {
    pub(crate) locals: HashMap<SymbolId, (u16, JvmSort)>,
    pub(crate) next_slot: u16,
    /// Exit labels of the enclosing `try ... finally` blocks, outermost first.
    /// A direct `return` from inside one has to run the finalizers before it
    /// leaves the method, so it jumps to the innermost exit instead of
    /// returning on the spot.
    pub(crate) finally_exits: Vec<Label>,
    /// Slot parking a `return` value while those finalizers run.
    pub(crate) return_slot: Option<u16>,
    pub(crate) tail_loop: Option<crate::gen_tailrec::TailLoop>,
}

impl Frame {
    pub(crate) fn instance() -> Self {
        Frame {
            locals: HashMap::new(),
            next_slot: 1,
            finally_exits: Vec::new(),
            return_slot: None,
            tail_loop: None,
        }
    }

    /// Slot for a `return` value that still has finalizers to run through.
    pub(crate) fn return_slot(&mut self, sort: JvmSort) -> u16 {
        match self.return_slot {
            Some(s) => s,
            None => {
                let s = self.alloc_tmp(sort);
                self.return_slot = Some(s);
                s
            }
        }
    }

    pub(crate) fn alloc(&mut self, id: SymbolId, sort: JvmSort) -> u16 {
        let slot = self.next_slot;
        if !id.is_none() {
            self.locals.insert(id, (slot, sort));
        }
        self.next_slot += sort.slots();
        slot
    }

    /// Allocate the slot an incoming *parameter* occupies. Identical to
    /// `alloc` except for `Unit`: the JVM really does pass a
    /// `scala/runtime/BoxedUnit` there, so the slot has to be reserved or
    /// every later parameter is read from the wrong index. The symbol itself
    /// stays void-sorted, so reading it leaves nothing on the stack the way
    /// every other `Unit` expression does (`load`/`store` of a void sort are
    /// no-ops) and nothing is lost: `BoxedUnit.UNIT` is the slot's only
    /// possible value and `emit_box` materialises it on demand.
    pub(crate) fn alloc_param(&mut self, id: SymbolId, sort: JvmSort, ty: &Type) -> u16 {
        let slot = self.next_slot;
        if !id.is_none() {
            self.locals.insert(id, (slot, sort));
        }
        self.next_slot += param_slots(ty).max(sort.slots());
        slot
    }

    pub(crate) fn alloc_tmp(&mut self, sort: JvmSort) -> u16 {
        let slot = self.next_slot;
        self.next_slot += sort.slots();
        slot
    }

    pub(crate) fn get(&self, id: SymbolId) -> Option<(u16, JvmSort)> {
        self.locals.get(&id).copied()
    }
}

pub(crate) struct ClassBuilder {
    pub(crate) access: u16,
    pub(crate) this_name: String,
    pub(crate) super_name: String,
    pub(crate) interfaces: Vec<String>,
    pub(crate) fields: Vec<Field>,
    pub(crate) methods: Vec<Method>,
    pub(crate) pool: Pool,
    pub(crate) source: String,
    pub(crate) scala_signature: Option<String>,
    pub(crate) scala_raw: bool,
    /// Class file format limits this class's members turned out not to fit in;
    /// see [`EmittedClass::format_errors`].
    pub(crate) format_errors: Vec<String>,
    /// JVMS §4.7.9 `Signature` on the class, and on its fields by name.
    pub(crate) signature: Option<String>,
    pub(crate) field_signatures: HashMap<String, String>,
    /// JVMS §4.7.2 `ConstantValue` on a `static final long` field
    /// (`@SerialVersionUID`).
    pub(crate) field_constants: HashMap<String, i64>,
}

impl ClassBuilder {
    pub(crate) fn new(this_name: String, source: &str) -> Self {
        ClassBuilder {
            access: ACC_PUBLIC | ACC_SUPER,
            this_name,
            super_name: "java/lang/Object".into(),
            interfaces: Vec::new(),
            fields: Vec::new(),
            methods: Vec::new(),
            pool: Pool::new(),
            source: source.to_string(),
            scala_signature: None,
            scala_raw: false,
            format_errors: Vec::new(),
            signature: None,
            field_signatures: HashMap::default(),
            field_constants: HashMap::default(),
        }
    }

    pub(crate) fn add_code(
        &mut self,
        access: u16,
        name: &str,
        desc: &str,
        max_locals: u16,
        gen: impl FnOnce(&mut Assembler),
    ) {
        let mut asm = Assembler::with_pool(std::mem::take(&mut self.pool), max_locals.max(1));
        asm.init_method(access, name, desc, &self.this_name);
        gen(&mut asm);
        let (code, pool) = asm.finish();
        self.pool = pool;
        if code.bytes.len() > MAX_CODE_LENGTH {
            // No encoding of this method exists (JVMS 4.7.3). nsc says the
            // same and emits nothing for the class; what we must not do is
            // write a `code_length` the loader rejects -- or, worse, let the
            // offsets that are still `u16` wrap and hand out a class file that
            // parses and then misbehaves.
            self.format_errors.push(format!(
                "Method too large: {}.{name} {desc}",
                self.this_name.replace('/', "."),
            ));
        }
        self.methods.push(Method {
            access,
            name: encode_method_name(name),
            desc: desc.to_string(),
            code: Some(code),
            java_annots: Vec::new(),
            signature: None,
        });
    }

    pub(crate) fn add_abstract(&mut self, access: u16, name: &str, desc: &str) {
        self.methods.push(Method {
            access,
            name: encode_method_name(name),
            desc: desc.to_string(),
            code: None,
            java_annots: Vec::new(),
            signature: None,
        });
    }

    /// Attach a JVMS §4.7.9 `Signature` to the method just emitted.
    ///
    /// Two conditions, both refusals rather than repairs (see
    /// [`crate::sig`]): a signature that *is* the descriptor says nothing, and
    /// one that does not erase back to the descriptor would contradict it.
    /// Either way the member keeps no attribute, which is what it had before
    /// this existed.
    pub(crate) fn sign_last(&mut self, sig: Option<&crate::sig::GenericSignature>) {
        let Some(g) = sig else { return };
        let Some(m) = self.methods.last_mut() else {
            return;
        };
        if g.sig == m.desc {
            return;
        }
        if crate::sig::erase_signature(&g.sig, &g.tvars).as_deref() != Some(m.desc.as_str()) {
            // Refusing is always safe, so a member that quietly loses its
            // signature leaves no trace. `SCALA_RS_SIG_DEBUG=1` prints the
            // rejects, which is how one finds the erasure disagreements that
            // are worth fixing next.
            if std::env::var_os("SCALA_RS_SIG_DEBUG").is_some() {
                eprintln!(
                    "SIGDROP {} {} sig={} erased={:?} tvars={:?}",
                    m.name,
                    m.desc,
                    g.sig,
                    crate::sig::erase_signature(&g.sig, &g.tvars),
                    g.tvars
                );
            }
            return;
        }
        m.signature = Some(g.sig.clone());
    }

    /// The same, for a field. `name` is the field's source name.
    pub(crate) fn sign_field(&mut self, name: &str, sig: Option<&crate::sig::GenericSignature>) {
        let Some(g) = sig else { return };
        let Some(f) = self.fields.iter().find(|f| f.name == name) else {
            return;
        };
        if g.sig == f.desc {
            return;
        }
        if crate::sig::erase_signature(&g.sig, &g.tvars).as_deref() != Some(f.desc.as_str()) {
            return;
        }
        self.field_signatures
            .insert(name.to_string(), g.sig.clone());
    }

    /// A method whose descriptor is a *prefix-extended* form of the signature's
    /// -- currently only the getter and setter synthesized for a `val`, whose
    /// type signature is recorded on the value symbol rather than on a method
    /// symbol of its own.
    pub(crate) fn sign_last_accessor(
        &mut self,
        sig: Option<&crate::sig::GenericSignature>,
        setter: bool,
    ) {
        let Some(g) = sig else { return };
        let wrapped = if setter {
            format!("({})V", g.sig)
        } else {
            format!("(){}", g.sig)
        };
        let g = crate::sig::GenericSignature {
            sig: wrapped,
            tvars: g.tvars.clone(),
            parents: Vec::new(),
        };
        self.sign_last(Some(&g));
    }

    /// Assemble and attach the class's own `Signature`: the formal type
    /// parameters recorded before erasure, then this class file's superclass
    /// and interfaces **in the order it lists them**, since
    /// `getGenericSuperclass` / `getGenericInterfaces` read them positionally.
    /// A parent with no recorded signature is written raw, which is what a
    /// Java class with no generic parent looks like and keeps the count right.
    pub(crate) fn sign_class(&mut self, sig: Option<&crate::sig::GenericSignature>) {
        let Some(g) = sig else { return };
        let mut out = g.sig.clone();
        let find = |n: &str| {
            g.parents
                .iter()
                .find(|(k, _)| k == n)
                .map(|(_, v)| v.clone())
                .unwrap_or_else(|| format!("L{n};"))
        };
        out.push_str(&find(&self.super_name));
        for i in &self.interfaces {
            out.push_str(&find(i));
        }
        // A signature with no formal parameters and no type argument anywhere
        // repeats the `super_class` / `interfaces` the class file already
        // states. nsc emits none in that case and neither should this.
        if !out.contains('<') {
            return;
        }
        let mut want = vec![self.super_name.clone()];
        want.extend(self.interfaces.iter().cloned());
        if crate::sig::erase_class_signature(&out, &g.tvars).as_deref() != Some(want.as_slice()) {
            return;
        }
        self.signature = Some(out);
    }

    pub(crate) fn add_java_annot_to_last(&mut self, desc: &str) {
        if let Some(m) = self.methods.last_mut() {
            if !m.java_annots.iter().any(|a| a == desc) {
                m.java_annots.push(desc.to_string());
            }
        }
    }

    /// Plain finish, with no `InnerClasses`/`EnclosingMethod` attribute.
    /// Used for classfiles with no corresponding [`SymbolId`] (synthetic
    /// helper classes: `DelayedInit` lambda
    /// bodies, user lambda closures) — nobody reflects on these, and a wrong
    /// guess from a name pattern is worse than omitting the attribute.
    pub(crate) fn finish(self) -> EmittedClass {
        self.finish_inner(Vec::new(), None)
    }

    /// Full finish: computes `InnerClasses` (self, if nested; every direct
    /// member of `extra_owner`'s class-like symbol, or of the symbol whose
    /// binary name matches this classfile's own `this_name` if that lookup
    /// succeeds; and every other nested class referenced anywhere in this
    /// classfile's own constant pool) plus `EnclosingMethod` (if the class
    /// itself turns out to be local or anonymous).
    ///
    /// `extra_owner` is consulted for the "list my own direct members" rule
    /// only when `this_name` has no matching symbol — e.g. the mirror class
    /// nsc calls a static forwarder: `Main` itself is never a symbol (only
    /// `Main`/`Main$` are), but it should still list the object's own
    /// nested classes, exactly as scalac's mirror class does.
    pub(crate) fn finish_full(
        mut self,
        st: &SymbolTable,
        jvm_index: &HashMap<String, SymbolId>,
        extra_owner: SymbolId,
    ) -> EmittedClass {
        // `write_with_pool` (called below, by `finish_inner`) is what
        // actually interns `super_name`/`interfaces` as `CONSTANT_Class`
        // entries — method bodies intern their own references as they are
        // assembled, but the superclass and interface list are still plain
        // strings at this point. Rule 3 of `compute_inner_classes` (scan the
        // pool for other referenced nested classes) needs them interned
        // first, or a class that only reaches a nested interface through
        // its `implements` clause (`class Circle extends Shape`) misses it.
        self.pool.class(&self.super_name);
        for i in &self.interfaces {
            self.pool.class(i);
        }
        let (inner_classes, enclosing_method) =
            compute_inner_classes(&self.this_name, &self.pool, st, jvm_index, extra_owner);
        self.finish_inner(inner_classes, enclosing_method)
    }

    pub(crate) fn finish_inner(
        self,
        inner_classes: Vec<InnerClassEntry>,
        enclosing_method: Option<EnclosingMethod>,
    ) -> EmittedClass {
        let this_name = self.this_name.clone();
        let class = ClassEmit {
            access: self.access,
            this_name: self.this_name,
            super_name: self.super_name,
            interfaces: self.interfaces,
            fields: self.fields,
            methods: self.methods,
            source: self.source,
            scala_signature: self.scala_signature,
            scala_raw: self.scala_raw,
            inner_classes,
            enclosing_method,
            signature: self.signature,
            field_signatures: self.field_signatures,
            field_constants: self.field_constants,
        };
        let bytes = class.write_with_pool(self.pool).expect("classfile write");
        EmittedClass {
            internal_name: this_name,
            bytes,
            format_errors: self.format_errors,
        }
    }
}

pub(crate) fn attach_scala_sig(
    b: &mut ClassBuilder,
    st: &SymbolTable,
    class_id: SymbolId,
    pickles: &HashMap<u32, Vec<u8>>,
) {
    if class_id.is_none() {
        return;
    }
    let raw = pickles
        .get(&class_id.0)
        .cloned()
        .unwrap_or_else(|| crate::pickle::pickle_class(st, class_id));
    if raw.is_empty() {
        return;
    }
    b.scala_signature = Some(crate::pickle::encode_to_annotation_string(&raw));
}

// ---------------------------------------------------------------------------
// InnerClasses (JVMS §4.7.6) / EnclosingMethod (JVMS §4.7.7)
// ---------------------------------------------------------------------------

/// JVMS §4.7.7 `EnclosingMethod`: the enclosing class's binary name, and —
/// if a method/constructor rather than a field initializer encloses the
/// class — that method's name and descriptor.
pub(crate) type EnclosingMethod = (String, Option<(String, String)>);

/// `getSimpleName`/`getEnclosingClass`/`isMemberClass`/`getDeclaringClass`
/// all read a class's own `InnerClasses` self-entry (and, for a local or
/// anonymous class, its `EnclosingMethod` attribute). Compute both for the
/// classfile named `this_name`:
///
/// 1. a self-entry, if `this_name` names a nested (member, local, or
///    anonymous) symbol;
/// 2. every direct class-like member of that symbol — or, failing that
///    lookup, of `extra_owner` (the mirror-class case, see
///    [`ClassBuilder::finish_full`]) — regardless of whether it is otherwise
///    used in this classfile's own bytecode: scalac always lists a class's
///    own declared nested classes, not just the ones it happens to
///    reference (verified against real scalac's `Outer`/`Outer$Level1`
///    classfiles, which list an unused nested `Level2`);
/// 3. every *other* nested class whose `CONSTANT_Class` already appears in
///    this classfile's own constant pool (`new`/`checkcast`/`instanceof`,
///    a field or method descriptor, the superclass or an interface, an
///    `$outer` field type, …) — JVMS §4.7.6's actual requirement.
pub(crate) fn compute_inner_classes(
    this_name: &str,
    pool: &Pool,
    st: &SymbolTable,
    jvm_index: &HashMap<String, SymbolId>,
    extra_owner: SymbolId,
) -> (Vec<InnerClassEntry>, Option<EnclosingMethod>) {
    let mut entries: Vec<InnerClassEntry> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut enclosing_method = None;

    let self_sym = jvm_index.get(this_name).copied();

    if let Some(id) = self_sym {
        if let Some((entry, encl)) = describe_nested(st, id, this_name) {
            seen.insert(entry.inner_class.clone());
            enclosing_method = encl;
            entries.push(entry);
        }
    }

    let members_owner = self_sym.filter(|id| !id.is_none()).unwrap_or(extra_owner);
    if !members_owner.is_none() {
        for m in st.get(members_owner).members.clone() {
            let mk = st.get(m).kind;
            if !matches!(mk, SymKind::Class | SymKind::ModuleClass) {
                continue;
            }
            let name = st.jvm_internal(m);
            if seen.contains(&name) {
                continue;
            }
            if let Some((entry, _)) = describe_nested(st, m, &name) {
                seen.insert(entry.inner_class.clone());
                entries.push(entry);
            }
        }
    }

    for name in pool.interned_class_names() {
        if seen.contains(&name) {
            continue;
        }
        let Some(&id) = jvm_index.get(&name) else {
            continue;
        };
        if let Some((entry, _)) = describe_nested(st, id, &name) {
            seen.insert(entry.inner_class.clone());
            entries.push(entry);
        }
    }

    entries.sort_by(|a, b| a.inner_class.cmp(&b.inner_class));
    (entries, enclosing_method)
}

/// Classify `id` (whose binary name is `name`) as a member, local, or
/// anonymous nested class, and build its `InnerClasses` entry — or `None`
/// if `id` is not nested at all (a top-level class/object). The second
/// element of the pair is only meaningful for a self-entry: the
/// `EnclosingMethod` this classfile needs if `id` is local/anonymous.
pub(crate) fn describe_nested(
    st: &SymbolTable,
    id: SymbolId,
    name: &str,
) -> Option<(InnerClassEntry, Option<EnclosingMethod>)> {
    let sym = st.get(id);
    let owner = sym.owner;
    if owner.is_none() {
        return None;
    }
    let owner_kind = st.get(owner).kind;
    if owner_kind == SymKind::Package {
        return None; // top-level: no InnerClasses entry at all.
    }
    // A local class's own binary name still gets a `$anon`-free simple
    // name; only literal anonymous classes (`new T { … }`) are unnamed.
    let is_anon = sym.name.starts_with("$anon$");
    let sflags = sym.flags;
    let mut flags = 0u16;
    if sflags.contains(Flags::PRIVATE) {
        flags |= ACC_PRIVATE;
    } else if sflags.contains(Flags::PROTECTED) {
        flags |= ACC_PROTECTED;
    } else {
        flags |= ACC_PUBLIC;
    }

    if is_anon || owner_kind == SymKind::Method {
        // Local or anonymous: JVMS says `outer_class_info_index` is zero.
        // scalac never sets `ACC_STATIC` here either way (matches real
        // scalac output for a local class in a module method).
        if is_anon || sflags.contains(Flags::FINAL) {
            flags |= ACC_FINAL;
        }
        let (enclosing_class, method_info) = if owner_kind == SymKind::Method {
            let mowner = st.get(owner).owner;
            (
                mowner,
                Some((st.get(owner).name.clone(), method_desc_from_sym(st, owner))),
            )
        } else {
            (owner, None)
        };
        if enclosing_class.is_none() {
            return None;
        }
        // A local class carries the disambiguating suffix in `inner_name`
        // too, not just in the binary name: nsc writes `Dog$1`, not `Dog`,
        // and `getSimpleName` reads this field. Derive it by removing the
        // enclosing class's prefix rather than re-deriving the index.
        let encl_internal = st.jvm_internal(enclosing_class);
        let inner_name = if is_anon {
            None
        } else {
            Some(
                name.strip_prefix(&encl_internal)
                    .map(|r| r.trim_start_matches('$').to_string())
                    .filter(|r| !r.is_empty())
                    .unwrap_or_else(|| sym.name.clone()),
            )
        };
        let entry = InnerClassEntry {
            inner_class: name.to_string(),
            outer_class: None,
            inner_name,
            access_flags: flags,
        };
        return Some((entry, Some((encl_internal, method_info))));
    }

    if !matches!(owner_kind, SymKind::Class | SymKind::ModuleClass) {
        // Unrecognized owner shape (e.g. a `Term`/`Module` symbol slipped
        // through) — be conservative and omit rather than guess.
        return None;
    }

    // Member class: `static` means "no `$outer` field" (nsc's optimization
    // for anything nested inside a module, which is a process-wide
    // singleton and so never needs one). `final` mirrors the source
    // modifier, except nsc never sets it for a module class itself (an
    // object's implicit `final` is not a written modifier).
    if outer_field_desc(st, id).is_none() {
        flags |= ACC_STATIC;
    }
    if sflags.contains(Flags::FINAL) && sym.kind != SymKind::ModuleClass {
        flags |= ACC_FINAL;
    }
    let entry = InnerClassEntry {
        inner_class: name.to_string(),
        outer_class: Some(st.jvm_internal(owner)),
        inner_name: Some(sym.name.clone()),
        access_flags: flags,
    };
    Some((entry, None))
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classfile::write_class_file;
    use scala_rs_typer::{has_errors, typecheck_str, typecheck_str_opts};
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempDir(PathBuf);

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn fresh_dir() -> TempDir {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let p = std::env::temp_dir().join(format!(
            "scala-rs-backend-{}-{}-{}",
            std::process::id(),
            n,
            nanos
        ));
        std::fs::create_dir_all(&p).expect("temp dir");
        TempDir(p)
    }

    fn java_available() -> bool {
        Command::new("java")
            .arg("-version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    fn write_classes(dir: &Path, classes: &[EmittedClass]) {
        for c in classes {
            let mut path = dir.to_path_buf();
            let parts: Vec<&str> = c
                .internal_name
                .split('/')
                .filter(|p| !p.is_empty())
                .collect();
            match parts.split_last() {
                Some((file, dirs)) => {
                    for d in dirs {
                        path.push(d);
                    }
                    path.push(format!("{file}.class"));
                }
                None => path.push(".class"),
            }
            write_class_file(&path, &c.bytes).expect("write class");
        }
    }

    fn compile_src(src: &str) -> Vec<EmittedClass> {
        let (mut tree, mut st, diags) = typecheck_str(src);
        assert!(
            !has_errors(&diags),
            "type errors: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        scala_rs_typer::uncurry(&mut tree, &mut st);
        scala_rs_typer::lambda_lift(&mut tree, &mut st);
        scala_rs_typer::erase(&mut tree, &mut st);
        let mut classes = crate::runtime::emit_runtime();
        classes.extend(emit(&tree, &st, "Test.scala").expect("backend emit"));
        classes
    }

    fn compile_src_library(src: &str) -> Vec<EmittedClass> {
        let (mut tree, mut st, diags) = typecheck_str_opts(
            src,
            &scala_rs_typer::TypecheckOptions {
                fatal_warnings: false,
                library_abi: true,
                classpath: Vec::new(),
                binary_path: Vec::new(),
                language_features: Vec::new(),
                ..scala_rs_typer::TypecheckOptions::default()
            },
        );
        assert!(
            !has_errors(&diags),
            "type errors: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        scala_rs_typer::uncurry(&mut tree, &mut st);
        scala_rs_typer::lambda_lift(&mut tree, &mut st);
        scala_rs_typer::erase(&mut tree, &mut st);
        emit_opts(
            &tree,
            &st,
            "Test.scala",
            EmitOpts {
                library_abi: true,
                ..Default::default()
            },
        )
        .expect("backend emit")
    }

    fn run_main(src: &str) -> Option<String> {
        if !java_available() {
            return None;
        }
        let classes = compile_src(src);
        assert!(!classes.is_empty(), "no classes emitted");
        let tmp = fresh_dir();
        write_classes(&tmp.0, &classes);
        let output = Command::new("java")
            .arg("-cp")
            .arg(&tmp.0)
            .arg("Main")
            .output()
            .expect("java");
        if !output.status.success() {
            let _ = Command::new("javap")
                .args(["-c", "-p", "-classpath"])
                .arg(&tmp.0)
                .arg("Main")
                .arg("Main$")
                .status();
            panic!(
                "java failed: status={:?}\nstdout:\n{}\nstderr:\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Some(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    #[test]
    fn emit_hello_class_names_and_magic() {
        let classes = compile_src(
            r#"
object Main {
  def main(args: Array[String]): Unit = println(1 + 2)
}
"#,
        );
        let names: Vec<&str> = classes.iter().map(|c| c.internal_name.as_str()).collect();
        assert!(names.contains(&"Main$"), "missing Main$ in {names:?}");
        assert!(names.contains(&"Main"), "missing Main in {names:?}");
        for c in &classes {
            assert!(
                c.bytes.len() >= 4 && c.bytes[0..4] == [0xCA, 0xFE, 0xBA, 0xBE],
                "{} is not a classfile",
                c.internal_name
            );
        }
    }

    #[test]
    fn dense_int_match_emits_tableswitch() {
        let classes = compile_src(
            r#"
import scala.annotation.switch
object Main {
  def dense(n: Int): Int = (n: @switch) match {
    case 0 => 10
    case 1 => 11
    case 2 => 12
    case 3 => 13
    case 4 => 14
  }
  def sparse(n: Int): Int = (n: @switch) match {
    case 0 => 1
    case 100 => 2
    case 200 => 3
  }
  def main(args: Array[String]): Unit = {
    println(dense(2))
    println(sparse(100))
  }
}
"#,
        );
        let main = classes
            .iter()
            .find(|c| c.internal_name == "Main$")
            .expect("Main$");
        assert!(
            main.bytes.contains(&0xaa),
            "dense Int match should emit tableswitch (0xaa)"
        );
        assert!(
            main.bytes.contains(&0xab),
            "sparse Int match should emit lookupswitch (0xab)"
        );
    }

    #[test]
    fn library_abi_does_not_emit_option_or_list() {
        let classes = compile_src_library(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val xs = 1 :: 2 :: Nil
    val o = Some(1)
    println(xs)
    println(o)
  }
}
"#,
        );
        for c in &classes {
            assert!(
                !c.internal_name.starts_with("scala/"),
                "library ABI must not emit {} (use scala-library.jar)",
                c.internal_name
            );
        }
        assert!(classes.iter().any(|c| c.internal_name == "Main$"));
    }

    #[test]
    fn hello_world_prints_3() {
        let Some(out) = run_main(
            r#"
object Main {
  def main(args: Array[String]): Unit = println(1 + 2)
}
"#,
        ) else {
            return;
        };
        assert!(
            out.contains('3') || out.to_lowercase().contains("hello"),
            "stdout: {out:?}"
        );
    }

    #[test]
    fn factorial_5_prints_120() {
        let Some(out) = run_main(
            r#"
object Main {
  def fact(n: Int): Int =
    if (n <= 1) 1 else n * fact(n - 1)
  def main(args: Array[String]): Unit = println(fact(5))
}
"#,
        ) else {
            return;
        };
        assert!(out.contains("120"), "stdout: {out:?}");
    }

    #[test]
    fn hello_fixture_string() {
        let Some(out) = run_main(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    println("hello, scala-rs")
  }
}
"#,
        ) else {
            return;
        };
        assert!(out.contains("hello, scala-rs"), "stdout: {out:?}");
    }

    #[test]
    fn arithmetic_fixture() {
        let Some(out) = run_main(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    println(1 + 2 * 3)
    println(10 - 4)
    println(20 / 4)
    println(7 % 3)
  }
}
"#,
        ) else {
            return;
        };
        assert!(out.contains('7'), "stdout: {out:?}");
        assert!(out.contains('6'), "stdout: {out:?}");
        assert!(out.contains('5'), "stdout: {out:?}");
        assert!(out.contains('1'), "stdout: {out:?}");
    }

    #[test]
    fn counter_class() {
        let Some(out) = run_main(
            r#"
class Counter(start: Int) {
  var n: Int = start
  def inc(): Unit = { n = n + 1 }
  def get(): Int = n
}
object Main {
  def main(args: Array[String]): Unit = {
    val c = new Counter(10)
    c.inc()
    c.inc()
    println(c.get())
  }
}
"#,
        ) else {
            return;
        };
        assert!(out.contains("12"), "stdout: {out:?}");
    }

    #[test]
    fn case_class_match() {
        let Some(out) = run_main(
            r#"
case class Point(x: Int, y: Int)
object Main {
  def main(args: Array[String]): Unit = {
    val p = Point(3, 4)
    println(p match {
      case Point(a, b) => a + b
    })
  }
}
"#,
        ) else {
            return;
        };
        assert!(out.contains('7'), "stdout: {out:?}");
    }

    #[test]
    fn trait_impl() {
        let Some(out) = run_main(
            r#"
trait Greeter {
  def greet(name: String): String
}
class HelloGreeter extends Greeter {
  def greet(name: String): String = "Hello, " + name
}
object Main {
  def main(args: Array[String]): Unit = {
    val g: Greeter = new HelloGreeter()
    println(g.greet("Scala"))
  }
}
"#,
        ) else {
            return;
        };
        assert!(out.contains("Hello, Scala"), "stdout: {out:?}");
    }

    #[test]
    fn while_loop() {
        let Some(out) = run_main(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    var i: Int = 0
    while (i < 3) {
      i = i + 1
    }
    println(i)
  }
}
"#,
        ) else {
            return;
        };
        assert!(out.contains('3'), "stdout: {out:?}");
    }

    #[test]
    fn string_interpolation() {
        let Some(out) = run_main(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val name = "world"
    println(s"hello $name")
    val n: Int = 7
    println(f"$n%02d")
    println(raw"a\nb")
  }
}
"#,
        ) else {
            return;
        };
        assert!(out.contains("hello world"), "stdout: {out:?}");
        assert!(out.contains("07"), "stdout: {out:?}");
        assert!(out.contains("a\\nb"), "stdout: {out:?}");
    }

    #[test]
    fn trait_concrete_method() {
        let Some(out) = run_main(
            r#"
trait T {
  def greet(): String = "from trait"
}
class C extends T
object Main {
  def main(args: Array[String]): Unit = {
    println(new C().greet())
  }
}
"#,
        ) else {
            return;
        };
        assert!(out.contains("from trait"), "stdout: {out:?}");
    }

    #[test]
    fn trait_linearization_last_mixin_wins() {
        let Some(out) = run_main(
            r#"
trait A {
  def msg: String = "A"
}
trait B {
  def msg: String = "B"
}
class C extends A with B
object Main {
  def main(args: Array[String]): Unit = {
    println(new C().msg)
  }
}
"#,
        ) else {
            return;
        };
        assert_eq!(out.trim(), "B", "stdout: {out:?}");
    }

    #[test]
    fn try_catch_finally_prints() {
        let Some(out) = run_main(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    try {
      println("before")
      throw new RuntimeException()
      println("after")
    } catch {
      case _: RuntimeException => println("caught")
    } finally {
      println("finally")
    }
  }
}
"#,
        ) else {
            return;
        };
        assert_eq!(out, "before\ncaught\nfinally\n", "stdout: {out:?}");
    }

    #[test]
    fn try_finally_success_and_throw() {
        let Some(out) = run_main(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    try {
      println("ok")
    } finally {
      println("fin-ok")
    }
    try {
      try {
        println("before-throw")
        throw new RuntimeException()
      } finally {
        println("fin-throw")
      }
    } catch {
      case _: RuntimeException => println("outer")
    }
    try {
      try {
        throw new RuntimeException()
      } catch {
        case _: RuntimeException =>
          println("caught")
          throw new RuntimeException()
      } finally {
        println("fin-catch")
      }
    } catch {
      case _: RuntimeException => println("outer2")
    }
  }
}
"#,
        ) else {
            return;
        };
        assert_eq!(
            out, "ok\nfin-ok\nbefore-throw\nfin-throw\nouter\ncaught\nfin-catch\nouter2\n",
            "stdout: {out:?}"
        );
    }

    #[test]
    fn update_assignment_array_and_user_def() {
        let Some(out) = run_main(
            r#"
class Cell {
  var n: Int = 0
  def update(i: Int, v: Int): Unit = { n = v + i }
  def apply(i: Int): Int = n + i
}
object Main {
  def main(args: Array[String]): Unit = {
    val arr = new Array[Int](2)
    arr(0) = 1
    arr(1) = 2
    println(arr(0))
    arr(1) = 9
    println(arr(1))
    val c = new Cell()
    c(1) = 10
    println(c.n)
    println(c(2))
  }
}
"#,
        ) else {
            return;
        };
        assert_eq!(out, "1\n9\n11\n13\n", "stdout: {out:?}");
    }

    #[test]
    fn nested_inner_class() {
        let Some(out) = run_main(
            r#"
class Outer {
  class Inner {
    def hi(): String = "inner"
  }
  def make(): String = new Inner().hi()
}
object Main {
  def main(args: Array[String]): Unit = {
    println(new Outer().make())
  }
}
"#,
        ) else {
            return;
        };
        assert!(out.contains("inner"), "stdout: {out:?}");
    }

    #[test]
    fn nested_object() {
        let Some(out) = run_main(
            r#"
object Outer {
  object Inner {
    def hi(): String = "nested"
  }
}
object Main {
  def main(args: Array[String]): Unit = {
    println(Outer.Inner.hi())
  }
}
"#,
        ) else {
            return;
        };
        assert!(out.contains("nested"), "stdout: {out:?}");
    }

    #[test]
    fn nonlocal_return_from_foreach_lambda() {
        let Some(out) = run_main(
            r#"
object Main {
  def find(xs: List[Int]): Int = {
    xs.foreach((x: Int) => { if (x > 0) return x })
    0
  }
  def nested: Int = {
    def inner: Int = { return 1 }
    inner
  }
  def main(args: Array[String]): Unit = {
    println(find(1 :: 2 :: Nil))
    println(find((-1) :: 3 :: Nil))
    println(nested)
  }
}
"#,
        ) else {
            return;
        };
        assert_eq!(out, "1\n3\n1\n", "stdout: {out:?}");
    }

    #[test]
    fn super_call_and_qualified_this() {
        let Some(out) = run_main(
            r#"
class Base {
  def greet(): String = "base"
}
trait T {
  def greet(): String = "T"
}
class C extends Base {
  def hi(): String = super.greet() + "!"
}
class D extends T {
  def hi(): String = super.greet() + "!"
}
class Outer {
  val name: String = "outer"
  class Inner {
    def who(): String = Outer.this.name
  }
  def inner(): String = new Inner().who()
}
object Main {
  def main(args: Array[String]): Unit = {
    println(new C().hi())
    println(new D().hi())
    println(new Outer().inner())
  }
}
"#,
        ) else {
            return;
        };
        assert_eq!(out, "base!\nT!\nouter\n", "stdout: {out:?}");
    }

    #[test]
    fn sealed_match_and_unapply() {
        let Some(out) = run_main(
            r#"
sealed trait Color
case class RGB(n: Int) extends Color
case object Black extends Color
object Even {
  def unapply(n: Int): Option[Int] = if (n % 2 == 0) Some(n / 2) else None
}
object Main {
  def show(c: Color): Int = c match {
    case RGB(n) => n
    case Black => 0
  }
  def main(args: Array[String]): Unit = {
    println(show(RGB(3)))
    println(show(Black))
    val x = 10 match {
      case Even(half) => half
      case _ => 0
    }
    println(x)
  }
}
"#,
        ) else {
            return;
        };
        assert_eq!(out, "3\n0\n5\n", "stdout: {out:?}");
    }

    #[test]
    fn value_class_erases_to_underlying() {
        let Some(out) = run_main(
            r#"
class Meter(val n: Int) extends AnyVal {
  def doubled: Int = n * 2
}
object Main {
  def main(args: Array[String]): Unit = {
    val m = new Meter(21)
    println(m.doubled)
    println(m.n)
  }
}
"#,
        ) else {
            return;
        };
        assert_eq!(out, "42\n21\n", "stdout: {out:?}");
    }

    #[test]
    fn predef_assert_arrow_stringops() {
        let Some(out) = run_main(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    assert(true)
    require(1 > 0)
    println("42".length)
    println("42".toInt)
    val t = 1 -> "a"
    println(t._1)
    println(t._2)
    try {
      ???
    } catch {
      case _: Throwable => println("nyi")
    }
  }
}
"#,
        ) else {
            return;
        };
        assert_eq!(out, "2\n42\n1\na\nnyi\n", "stdout: {out:?}");
    }

    #[test]
    fn unapply_seq_list_and_named() {
        let Some(out) = run_main(
            r#"
object PairSeq {
  def unapplySeq(n: Int): Option[List[Int]] = Some(n :: (n + 1) :: Nil)
}
case class Point(x: Int, y: Int)
object Main {
  def main(args: Array[String]): Unit = {
    val xs = 1 :: 2 :: 3 :: Nil
    val s = xs match {
      case List(a, b, c) => a + b + c
      case _ => 0
    }
    println(s)
    val t = 10 match {
      case PairSeq(a, b) => a + b
      case _ => -1
    }
    println(t)
    val h = xs match {
      case List(a, rest @ _*) => a
      case _ => 0
    }
    println(h)
    val p = Point(3, 4) match {
      case Point(y = b, x = a) => a + b
      case _ => 0
    }
    println(p)
  }
}
"#,
        ) else {
            return;
        };
        assert_eq!(out, "6\n21\n1\n7\n", "stdout: {out:?}");
    }

    #[test]
    fn trait_val_init() {
        let Some(out) = run_main(
            r#"
trait T {
  val msg: String = "from trait"
}
class C extends T
object Main {
  def main(args: Array[String]): Unit = {
    println(new C().msg)
  }
}
"#,
        ) else {
            return;
        };
        assert_eq!(out.trim(), "from trait", "stdout: {out:?}");
    }

    #[test]
    fn abstract_override_super_chain() {
        let Some(out) = run_main(
            r#"
trait Base {
  def msg: String = "base"
}
trait A extends Base {
  abstract override def msg: String = "A-" + super.msg
}
trait B extends Base {
  abstract override def msg: String = "B-" + super.msg
}
class C extends Base with A with B
object Main {
  def main(args: Array[String]): Unit = {
    println(new C().msg)
  }
}
"#,
        ) else {
            return;
        };
        assert_eq!(out.trim(), "B-A-base", "stdout: {out:?}");
    }

    #[test]
    fn predef_identity_locally_implicitly_stringadd() {
        let Some(out) = run_main(
            r#"
object Main {
  implicit val n: Int = 41
  def main(args: Array[String]): Unit = {
    println(1 + "x")
    println(implicitly[Int])
    println(identity(42))
    locally {
      println("here")
    }
  }
}
"#,
        ) else {
            return;
        };
        assert_eq!(out, "1x\n41\n42\nhere\n", "stdout: {out:?}");
    }
}
