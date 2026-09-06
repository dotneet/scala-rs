//! Function values and the operator-shaped expressions: the capture analysis
//! behind them (boxed locals, free variables), `PartialFunction` classes,
//! `invokedynamic` lambdas with their hoisted bodies, primitive and reference
//! comparison, `synchronized`, string concatenation and interpolation, and
//! `try` / `catch` / `finally`.

use crate::classfile::{EmittedClass, ACC_INTERFACE, ACC_STATIC};
use crate::classfile::{Field, ACC_FINAL, ACC_PUBLIC, ACC_SUPER, ACC_SYNTHETIC};
use crate::code::{Assembler, StackEntry};
use crate::gen::*;
use scala_rs_parser::Flags;
use scala_rs_parser::{Lit, SymbolId, Tree, TreeKind, Type};
use scala_rs_typer::SymKind;
use scala_rs_typer::SymbolTable;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

pub(crate) fn collect_boxed_vars(
    tree: &Tree,
    st: &SymbolTable,
    captured: Option<&Rc<HashSet<SymbolId>>>,
) -> HashSet<SymbolId> {
    let mut out = HashSet::new();
    walk_boxed_vars(tree, st, &mut out);
    match captured {
        Some(shared) => out.extend(shared.iter().copied()),
        None => out.extend(collect_captured_vars(st).iter().copied()),
    }
    out
}

/// Every mutable local captured by a class defined inside a method: those are
/// shared with the enclosing method, exactly like one captured by a lambda, so
/// they are boxed.
///
/// A function of the symbol table alone, which does not change while a run is
/// emitted. The driver calls this once and hands the answer to every unit
/// through [`EmitOpts::captured_vars`]; reading every symbol's `captures` once
/// per unit was 184 sweeps of ~100k symbols on slick.
pub fn collect_captured_vars(st: &SymbolTable) -> HashSet<SymbolId> {
    let mut out = HashSet::new();
    for s in &st.symbols {
        for c in &s.captures {
            if st.get(*c).flags.contains(Flags::MUTABLE) {
                out.insert(*c);
            }
        }
    }
    out
}

pub(crate) fn walk_boxed_vars(tree: &Tree, st: &SymbolTable, out: &mut HashSet<SymbolId>) {
    match &tree.kind {
        TreeKind::PackageDef { stats, .. } => {
            for s in stats {
                walk_boxed_vars(s, st, out);
            }
        }
        TreeKind::ClassDef {
            vparamss, impl_, ..
        } => {
            for p in vparamss.iter().flatten() {
                walk_boxed_vars(p, st, out);
            }
            for p in &impl_.parents {
                walk_boxed_vars(p, st, out);
            }
            for s in &impl_.body {
                walk_boxed_vars(s, st, out);
            }
        }
        TreeKind::ModuleDef { impl_, .. } => {
            for p in &impl_.parents {
                walk_boxed_vars(p, st, out);
            }
            for s in &impl_.body {
                walk_boxed_vars(s, st, out);
            }
        }
        TreeKind::DefDef {
            vparamss, tpt, rhs, ..
        } => {
            let synthetic = def_is_synthetic(st, tree);
            for p in vparamss.iter().flatten() {
                if synthetic && !p.sym.is_none() && st.get(p.sym).flags.contains(Flags::MUTABLE) {
                    out.insert(p.sym);
                }
                walk_boxed_vars(p, st, out);
            }
            walk_boxed_vars(tpt, st, out);
            walk_boxed_vars(rhs, st, out);
        }
        TreeKind::Function { vparams, body } => {
            let mut bound = HashSet::new();
            for p in vparams {
                if !p.sym.is_none() {
                    bound.insert(p.sym);
                }
                walk_boxed_vars(p, st, out);
            }
            let mut free = FreeVars::default();
            collect_free(body, &bound, &mut free, st);
            for id in free.vars {
                let s = st.get(id);
                if s.flags.contains(Flags::MUTABLE)
                    && matches!(st.get(s.owner).kind, SymKind::Method)
                {
                    out.insert(id);
                }
            }
            walk_boxed_vars(body, st, out);
        }
        TreeKind::ValDef { tpt, rhs, .. } => {
            walk_boxed_vars(tpt, st, out);
            walk_boxed_vars(rhs, st, out);
        }
        TreeKind::Block { stats, expr } => {
            for s in stats {
                walk_boxed_vars(s, st, out);
            }
            walk_boxed_vars(expr, st, out);
        }
        TreeKind::If { cond, thenp, elsep } => {
            walk_boxed_vars(cond, st, out);
            walk_boxed_vars(thenp, st, out);
            walk_boxed_vars(elsep, st, out);
        }
        TreeKind::While { cond, body } | TreeKind::DoWhile { cond, body } => {
            walk_boxed_vars(cond, st, out);
            walk_boxed_vars(body, st, out);
        }
        TreeKind::Apply { fun, args } | TreeKind::TypeApply { fun, args } => {
            walk_boxed_vars(fun, st, out);
            for a in args {
                walk_boxed_vars(a, st, out);
            }
        }
        TreeKind::Typed { expr, tpt } => {
            walk_boxed_vars(expr, st, out);
            walk_boxed_vars(tpt, st, out);
        }
        TreeKind::Select { qual, .. } => walk_boxed_vars(qual, st, out),
        TreeKind::Assign { lhs, rhs } => {
            walk_boxed_vars(lhs, st, out);
            walk_boxed_vars(rhs, st, out);
        }
        TreeKind::Match { selector, cases } => {
            walk_boxed_vars(selector, st, out);
            for c in cases {
                walk_boxed_vars(&c.pat, st, out);
                walk_boxed_vars(&c.guard, st, out);
                walk_boxed_vars(&c.body, st, out);
            }
        }
        TreeKind::Try {
            block,
            catches,
            finalizer,
        } => {
            walk_boxed_vars(block, st, out);
            for c in catches {
                walk_boxed_vars(&c.pat, st, out);
                walk_boxed_vars(&c.body, st, out);
            }
            walk_boxed_vars(finalizer, st, out);
        }
        TreeKind::Return { expr } | TreeKind::Throw { expr } | TreeKind::New { tpt: expr } => {
            walk_boxed_vars(expr, st, out);
        }
        TreeKind::InterpolatedString { args, .. } => {
            for a in args {
                walk_boxed_vars(a, st, out);
            }
        }
        _ => {}
    }
}

/// What a lambda body reaches for outside itself: the enclosing method's locals
/// it captures, and whether it also needs the enclosing *instance*.
#[derive(Default)]
pub(crate) struct FreeVars {
    pub(crate) vars: Vec<SymbolId>,
    /// The body mentions the enclosing `this` — written out (`this.f`, `super.f`)
    /// or, far more often, left implicit in a call to a method of the enclosing
    /// class. Without this the lambda class gets no `$outer` and codegen's
    /// `load_this` reads slot 0, which inside `apply` is the *lambda*:
    /// `C$$anonfun$0 cannot be cast to C` at runtime.
    pub(crate) uses_this: bool,
}

/// Does an `Ident` naming `id` compile to a call on the enclosing instance?
/// Members of an *object* do not — they go through `MODULE$` — and neither do
/// a nested `def`'s locals, whose owner is a method.
pub(crate) fn ident_reads_enclosing_this(st: &SymbolTable, id: SymbolId) -> bool {
    let s = st.get(id);
    if s.kind != SymKind::Method {
        return false;
    }
    matches!(st.get(s.owner).kind, SymKind::Class)
}

pub(crate) fn collect_free(
    tree: &Tree,
    bound: &HashSet<SymbolId>,
    out: &mut FreeVars,
    st: &SymbolTable,
) {
    match &tree.kind {
        TreeKind::Ident { .. } => {
            if !tree.sym.is_none() && !bound.contains(&tree.sym) {
                let s = st.get(tree.sym);
                if s.kind == SymKind::Term && !out.vars.contains(&tree.sym) {
                    out.vars.push(tree.sym);
                }
                if ident_reads_enclosing_this(st, tree.sym) {
                    out.uses_this = true;
                }
            }
        }
        TreeKind::Function { vparams, body } => {
            let mut b = bound.clone();
            for p in vparams {
                if !p.sym.is_none() {
                    b.insert(p.sym);
                }
            }
            collect_free(body, &b, out, st);
        }
        TreeKind::Super { .. } | TreeKind::This { .. } => out.uses_this = true,
        TreeKind::Select { qual, .. } => collect_free(qual, bound, out, st),
        TreeKind::UnApply { fun, args } => {
            collect_free(fun, bound, out, st);
            for a in args {
                collect_free(a, bound, out, st);
            }
        }
        TreeKind::Apply { fun, args } => {
            collect_free(fun, bound, out, st);
            for a in args {
                collect_free(a, bound, out, st);
            }
        }
        TreeKind::Block { stats, expr } => {
            let mut b = bound.clone();
            for s in stats {
                if let TreeKind::ValDef { .. } = &s.kind {
                    if !s.sym.is_none() {
                        b.insert(s.sym);
                    }
                }
                collect_free(s, &b, out, st);
            }
            collect_free(expr, &b, out, st);
        }
        TreeKind::If { cond, thenp, elsep } => {
            collect_free(cond, bound, out, st);
            collect_free(thenp, bound, out, st);
            collect_free(elsep, bound, out, st);
        }
        TreeKind::ValDef { rhs, .. } => collect_free(rhs, bound, out, st),
        TreeKind::Assign { lhs, rhs } => {
            collect_free(lhs, bound, out, st);
            collect_free(rhs, bound, out, st);
        }
        TreeKind::Typed { expr, .. } | TreeKind::TypeApply { fun: expr, .. } => {
            collect_free(expr, bound, out, st);
        }
        TreeKind::While { cond, body } | TreeKind::DoWhile { cond, body } => {
            collect_free(cond, bound, out, st);
            collect_free(body, bound, out, st);
        }
        TreeKind::Match { selector, cases } => {
            collect_free(selector, bound, out, st);
            for c in cases {
                collect_free(&c.pat, bound, out, st);
                collect_free(&c.body, bound, out, st);
                collect_free(&c.guard, bound, out, st);
            }
        }
        TreeKind::InterpolatedString { args, .. } => {
            for a in args {
                collect_free(a, bound, out, st);
            }
        }
        TreeKind::Try {
            block,
            catches,
            finalizer,
        } => {
            collect_free(block, bound, out, st);
            for c in catches {
                collect_free(&c.pat, bound, out, st);
                collect_free(&c.body, bound, out, st);
                collect_free(&c.guard, bound, out, st);
            }
            collect_free(finalizer, bound, out, st);
        }
        TreeKind::Return { expr } | TreeKind::Throw { expr } => {
            collect_free(expr, bound, out, st);
        }
        TreeKind::New { tpt } => {
            // Instantiating a class that captures enclosing-method locals reads
            // those locals right here.
            if let Some(cid) = new_class_sym(st, tpt) {
                for c in &st.get(cid).captures {
                    if !bound.contains(c) && !out.vars.contains(c) {
                        out.vars.push(*c);
                    }
                }
                // …and a nested class's `<init>` takes the enclosing instance,
                // which the lambda can only hand over if it kept one. The
                // anonymous class's *body* is a `ClassDef` this walk does not
                // descend into, so `new AnyRef { … outerField … }` inside a
                // lambda looks free of `this` while its `$outer` argument is
                // exactly `this`; without the field, `load_this` pushed the
                // lambda itself and the verifier rejected `<init>`.
                if outer_field_class(st, cid).is_some() {
                    out.uses_this = true;
                }
            }
            collect_free(tpt, bound, out, st);
        }
        _ => {}
    }
}

/// Class symbol instantiated by `new <tpt>`, if it is known.
pub(crate) fn new_class_sym(st: &SymbolTable, tpt: &Tree) -> Option<SymbolId> {
    if !tpt.sym.is_none() && st.get(tpt.sym).is_class_like() {
        return Some(tpt.sym);
    }
    st.class_sym_of(&tpt.ty)
}

pub(crate) fn is_partial_function_ty(st: &SymbolTable, ty: &Type) -> bool {
    match ty {
        Type::Named { name, .. } if name == "PartialFunction" => true,
        Type::Class { sym, .. } => {
            let s = st.get(*sym);
            s.name == "PartialFunction" && s.jvm_name.contains("PartialFunction")
        }
        _ => false,
    }
}

pub(crate) fn pf_match_cases(body: &Tree) -> Option<&[scala_rs_parser::CaseDef]> {
    match &body.kind {
        TreeKind::Match { cases, .. } => Some(cases),
        TreeKind::Block { expr, .. } => pf_match_cases(expr),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_partial_function_methods<'a>(
    b: &mut ClassBuilder,
    st: &'a SymbolTable,
    extras: &'a RefCell<Vec<EmittedClass>>,
    lambda_n: &'a Cell<u32>,
    ctx_bodies: &'a RefCell<Vec<PendingBody>>,
    ctx_hoist: Option<&'a str>,
    source: &'a str,
    class_sym: SymbolId,
    library_abi: bool,
    orig_class: &str,
    lam_name: &str,
    outer_desc: &str,
    need_outer: bool,
    vparams: &[Tree],
    body: &Tree,
    local_caps: &[SymbolId],
    ret_ty: &Type,
    boxed_vars: &HashSet<SymbolId>,
    emit_errors: Rc<RefCell<Vec<EmitError>>>,
) {
    let cases: Vec<scala_rs_parser::CaseDef> = pf_match_cases(body).unwrap_or(&[]).to_vec();
    let sel_ty = match &body.kind {
        TreeKind::Match { selector, .. } => selector.ty.clone(),
        _ => vparams.first().map(|p| p.ty.clone()).unwrap_or(Type::Any),
    };
    let vparams = vparams.to_vec();
    let local_caps = local_caps.to_vec();
    let lam_name = lam_name.to_string();
    let outer_desc = outer_desc.to_string();
    let orig_class = orig_class.to_string();
    let ret_ty = ret_ty.clone();

    let cases1 = cases.clone();
    let vparams1 = vparams.clone();
    let caps1 = local_caps.clone();
    let lam1 = lam_name.clone();
    let outer1 = outer_desc.clone();
    let orig1 = orig_class.clone();
    let sel1 = sel_ty.clone();

    b.add_code(ACC_PUBLIC, "isDefinedAt", "(Ljava/lang/Object;)Z", 8, |a| {
        let mut fr = Frame::instance();
        fr.next_slot = 2;
        pf_bind_arg_and_captures(a, &mut fr, st, &lam1, &vparams1, &caps1, boxed_vars);
        let some = a.fresh_label();
        let no = a.fresh_label();
        let sel_sort = jvm_sort(&sel1);
        if let Some(p) = vparams1.first() {
            if let Some((slot, sort)) = fr.get(p.sym) {
                load(a, slot, sort);
                let tmp = fr.alloc_tmp(sel_sort);
                store(a, tmp, sel_sort);
                for c in &cases1 {
                    let fail = a.fresh_label();
                    let outer_storage = (lam1.as_str(), "$outer", outer1.as_str());
                    let outer_ref = if need_outer {
                        Some(outer_storage)
                    } else {
                        None
                    };
                    let inner_ctx = EmitCtx {
                        st,
                        class_sym,
                        class_name: &orig1,
                        ret_ty: Type::Boolean,
                        emit_errors: std::rc::Rc::clone(&emit_errors),
                        extras,
                        lambda_n,
                        lambda_bodies: ctx_bodies,
                        hoist_owner: ctx_hoist,
                        source,
                        outer: outer_ref,
                        presuper_outer: None,
                        outer_slot: None,
                        library_abi,
                        method_sym: SymbolId::NONE,
                        boxed_vars,
                        value_ext: None,
                    };
                    gen_pattern(a, &mut fr, &inner_ctx, &c.pat, tmp, sel_sort, fail);
                    if !c.guard.is_empty() {
                        gen_expr(a, &mut fr, &inner_ctx, &c.guard);
                        a.ifeq(fail);
                    }
                    a.goto(some);
                    a.mark(fail);
                }
            }
        }
        a.goto(no);
        a.mark(some);
        a.iconst(1);
        a.ireturn();
        a.mark(no);
        a.iconst(0);
        a.ireturn();
    });

    b.add_code(
        ACC_PUBLIC,
        "applyOrElse",
        "(Ljava/lang/Object;Lscala/Function1;)Ljava/lang/Object;",
        8,
        |a| {
            let mut fr = Frame::instance();
            fr.next_slot = 3;
            pf_bind_arg_and_captures(a, &mut fr, st, &lam_name, &vparams, &local_caps, boxed_vars);
            let end = a.fresh_label();
            let sel_sort = jvm_sort(&sel_ty);
            if let Some(p) = vparams.first() {
                if let Some((slot, sort)) = fr.get(p.sym) {
                    load(a, slot, sort);
                    let tmp = fr.alloc_tmp(sel_sort);
                    store(a, tmp, sel_sort);
                    for c in &cases {
                        let fail = a.fresh_label();
                        let outer_storage = (lam_name.as_str(), "$outer", outer_desc.as_str());
                        let outer_ref = if need_outer {
                            Some(outer_storage)
                        } else {
                            None
                        };
                        let inner_ctx = EmitCtx {
                            st,
                            class_sym,
                            class_name: &orig_class,
                            ret_ty: ret_ty.clone(),
                            emit_errors: std::rc::Rc::clone(&emit_errors),
                            extras,
                            lambda_n,
                            lambda_bodies: ctx_bodies,
                            hoist_owner: ctx_hoist,
                            source,
                            outer: outer_ref,
                            presuper_outer: None,
                            outer_slot: None,
                            library_abi,
                            method_sym: SymbolId::NONE,
                            boxed_vars,
                            value_ext: None,
                        };
                        gen_pattern(a, &mut fr, &inner_ctx, &c.pat, tmp, sel_sort, fail);
                        if !c.guard.is_empty() {
                            gen_expr(a, &mut fr, &inner_ctx, &c.guard);
                            a.ifeq(fail);
                        }
                        if is_unit_like(&ret_ty) {
                            gen_stat(a, &mut fr, &inner_ctx, &c.body);
                            emit_box(a, &Type::Unit);
                        } else {
                            gen_expr(a, &mut fr, &inner_ctx, &c.body);
                            emit_box(a, &ret_ty);
                        }
                        a.goto(end);
                        a.mark(fail);
                    }
                }
            }
            // The synthetic `apply` above passes `null` for the fallback: no
            // case matched and there is nothing to fall back to, which is
            // exactly `MatchError`. Every real caller (`collect`, `orElse`,
            // `lift`) passes a function and is unaffected.
            let has_default = a.fresh_label();
            a.aload(2);
            a.ifnonnull(has_default);
            a.new_obj("scala/MatchError");
            a.dup();
            a.aload(1);
            a.invokespecial("scala/MatchError", "<init>", "(Ljava/lang/Object;)V");
            a.athrow();
            a.mark(has_default);
            a.aload(2);
            a.aload(1);
            a.invokeinterface(
                "scala/Function1",
                "apply",
                "(Ljava/lang/Object;)Ljava/lang/Object;",
            );
            a.mark(end);
            a.areturn();
        },
    );
}

pub(crate) fn pf_bind_arg_and_captures(
    a: &mut Assembler,
    fr: &mut Frame,
    st: &SymbolTable,
    lam_name: &str,
    vparams: &[Tree],
    local_caps: &[SymbolId],
    boxed: &HashSet<SymbolId>,
) {
    if let Some(p) = vparams.first() {
        a.aload(1);
        if is_jvm_primitive(&p.ty) || matches!(p.ty, Type::String) {
            emit_unbox(a, &p.ty);
        } else if let Type::Class { sym, .. } = &p.ty {
            let n = class_internal(st, *sym);
            if !n.is_empty() && n != "java/lang/Object" {
                a.checkcast(&n);
            }
        } else {
            emit_unbox(a, &p.ty);
        }
        let sort = jvm_sort(&p.ty);
        let slot = fr.alloc(p.sym, sort);
        store(a, slot, sort);
    }
    for (i, id) in local_caps.iter().enumerate() {
        let ty = st.get(*id).ty.clone();
        a.aload(0);
        a.getfield(lam_name, &format!("$captured${i}"), "Ljava/lang/Object;");
        if boxed.contains(id) {
            a.checkcast(runtime_ref_class(&ty));
            let slot = fr.alloc(*id, JvmSort::Ref);
            store(a, slot, JvmSort::Ref);
        } else {
            emit_from_erased_object(a, st, &ty);
            let sort = jvm_sort(&ty);
            let slot = fr.alloc(*id, sort);
            store(a, slot, sort);
        }
    }
}

/// `scala.FunctionN` only goes up to 22; beyond that there is no functional
/// interface for `LambdaMetafactory` to implement.
pub(crate) const MAX_FUNCTION_ARITY: usize = 22;

/// The erased descriptor of `scala.FunctionN.apply`: `(Object…)Object`.
pub(crate) fn erased_apply_desc(arity: usize) -> String {
    let mut d = String::from("(");
    for _ in 0..arity {
        d.push_str("Ljava/lang/Object;");
    }
    d.push_str(")Ljava/lang/Object;");
    d
}

/// Emit a `FunctionN` literal as an `invokedynamic` bound to
/// `LambdaMetafactory.metafactory`, and queue its body to become a static
/// method of `owner`.
///
/// The call site's descriptor is `(<captured values>)L<FunctionN>;`, so the
/// captures are the only thing pushed here; the JDK spins the closure class
/// at link time and no classfile is written for it. The body method is
/// written at the *erased* shape — every parameter and the result are
/// `java/lang/Object` — which makes `samMethodType`, `instantiatedMethodType`
/// and the implementation's own signature identical and leaves
/// `LambdaMetafactory` nothing to adapt.
#[allow(clippy::too_many_arguments)]
pub(crate) fn gen_function_indy(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    owner: &str,
    n: u32,
    iface: &str,
    arity: usize,
    need_outer: bool,
    local_caps: &[SymbolId],
    vparams: &[Tree],
    body: &Tree,
    fn_ty: &Type,
) {
    // Before invokespecial <init>, slot zero is uninitializedThis and
    // cannot be passed to LambdaMetafactory. The available receiver is the
    // enclosing instance supplied as the constructor's outer parameter.
    let (outer_class, outer_sym) =
        if let Some((_, enclosing, held)) = ctx.presuper_outer.filter(|_| need_outer) {
            (class_internal(ctx.st, held), enclosing)
        } else {
            (ctx.class_name.to_string(), ctx.class_sym)
        };
    let outer_desc = format!("L{outer_class};");
    let mut call_desc = String::from("(");
    if need_outer {
        if let Some((slot, _, _)) = ctx.presuper_outer {
            load(asm, slot, JvmSort::Ref);
        } else {
            load_this(asm, ctx);
        }
        call_desc.push_str(&outer_desc);
    }
    for id in local_caps {
        let (slot, sort) = frame.get(*id).expect("captured local has a slot");
        let ty = ctx.st.get(*id).ty.clone();
        if is_boxed_var(ctx, *id) {
            // Capture the IntRef/ObjectRef itself, not the elem.
            load(asm, slot, JvmSort::Ref);
        } else {
            load(asm, slot, sort);
            if is_jvm_primitive(&ty) {
                emit_box(asm, &ty);
            }
        }
        call_desc.push_str("Ljava/lang/Object;");
    }
    call_desc.push_str(&format!(")L{iface};"));

    let sam_desc = erased_apply_desc(arity);
    let mut impl_desc = String::from("(");
    if need_outer {
        impl_desc.push_str(&outer_desc);
    }
    for _ in local_caps {
        impl_desc.push_str("Ljava/lang/Object;");
    }
    for _ in 0..arity {
        impl_desc.push_str("Ljava/lang/Object;");
    }
    impl_desc.push_str(")Ljava/lang/Object;");
    let impl_name = format!("$anonfun${n}");
    // A lambda in a trait's `default` method (or in its `$init$`) hoists onto
    // the interface, and the method handle then has to be an
    // `InterfaceMethodref`.
    let owner_is_iface = owner == ctx.class_name && is_interface_sym(ctx.st, ctx.class_sym);
    asm.invokedynamic_lambda(
        "apply",
        &sam_desc,
        &call_desc,
        owner,
        &impl_name,
        &impl_desc,
        owner_is_iface,
    );

    ctx.lambda_bodies.borrow_mut().push(PendingBody {
        name: impl_name,
        desc: impl_desc,
        has_outer: need_outer,
        outer_class,
        class_sym: outer_sym,
        vparams: vparams.to_vec(),
        body: body.clone(),
        local_caps: local_caps.to_vec(),
        ret_ty: match fn_ty {
            Type::Function { ret, .. } => (**ret).clone(),
            t => t.clone(),
        },
    });
}

/// Write one queued lambda body as a static method of `b`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_lambda_body(
    b: &mut ClassBuilder,
    st: &SymbolTable,
    extras: &RefCell<Vec<EmittedClass>>,
    lambda_n: &Cell<u32>,
    lambda_bodies: &RefCell<Vec<PendingBody>>,
    source: &str,
    library_abi: bool,
    boxed: &HashSet<SymbolId>,
    emit_errors: Rc<RefCell<Vec<EmitError>>>,
    pb: PendingBody,
) {
    // nsc's own `$anonfun$` methods are `public static final synthetic`;
    // public matters because a lambda nested inside a closure class links its
    // `invokedynamic` to a body method that lives on a *different* class.
    // On an interface — a lambda inside a trait's `default` method hoists
    // there, exactly as nsc's do — JVMS §4.6 forbids `ACC_FINAL`.
    let access = if b.access & ACC_INTERFACE != 0 {
        ACC_PUBLIC | ACC_STATIC | ACC_SYNTHETIC
    } else {
        ACC_PUBLIC | ACC_STATIC | ACC_FINAL | ACC_SYNTHETIC
    };
    let owner = b.this_name.clone();
    let base = u16::from(pb.has_outer);
    let n_caps = pb.local_caps.len() as u16;
    let n_params = base + n_caps + pb.vparams.len() as u16;
    let name = pb.name.clone();
    let desc = pb.desc.clone();
    b.add_code(access, &name, &desc, n_params + 8, move |a| {
        let mut fr = Frame::instance();
        fr.next_slot = n_params;
        for (i, p) in pb.vparams.iter().enumerate() {
            let obj_slot = base + n_caps + i as u16;
            // A parameter instantiated at a value class receives the boxed
            // instance; erasure recorded that on the symbol.
            let ty = if p.sym.is_none() {
                p.ty.clone()
            } else {
                st.get(p.sym).ty.clone()
            };
            a.aload(obj_slot);
            unerase_lambda_param(a, st, &ty);
            let sort = jvm_sort(&ty);
            let slot = fr.alloc(p.sym, sort);
            store(a, slot, sort);
        }
        for (i, id) in pb.local_caps.iter().enumerate() {
            let ty = st.get(*id).ty.clone();
            a.aload(base + i as u16);
            if boxed.contains(id) {
                a.checkcast(runtime_ref_class(&ty));
                let slot = fr.alloc(*id, JvmSort::Ref);
                store(a, slot, JvmSort::Ref);
            } else {
                emit_from_erased_object(a, st, &ty);
                let sort = jvm_sort(&ty);
                let slot = fr.alloc(*id, sort);
                store(a, slot, sort);
            }
        }
        let inner_ctx = EmitCtx {
            st,
            class_sym: pb.class_sym,
            class_name: &pb.outer_class,
            ret_ty: pb.ret_ty.clone(),
            emit_errors: std::rc::Rc::clone(&emit_errors),
            extras,
            lambda_n,
            lambda_bodies,
            hoist_owner: Some(&owner),
            source,
            outer: None,
            presuper_outer: None,
            outer_slot: if pb.has_outer { Some(0) } else { None },
            library_abi,
            method_sym: SymbolId::NONE,
            boxed_vars: boxed,
            value_ext: None,
        };
        gen_expr(a, &mut fr, &inner_ctx, &pb.body);
        if matches!(pb.body.ty, Type::Nothing) {
            // `throw` already emitted athrow; an areturn here would be an
            // unreachable StackMapTable target.
        } else if is_unit_like(&pb.ret_ty) {
            pop_if_value(a, &pb.body.ty);
            emit_box(a, &Type::Unit);
            a.areturn();
        } else {
            emit_box(a, &pb.ret_ty);
            a.areturn();
        }
    });
}

/// Bring a lambda parameter back from the erased `Object` slot the SAM hands
/// it in. Shared by the `invokedynamic` body and the anonymous-class `apply`.
pub(crate) fn unerase_lambda_param(a: &mut Assembler, st: &SymbolTable, ty: &Type) {
    if is_jvm_primitive(ty) || matches!(ty, Type::String) {
        emit_unbox(a, ty);
    } else if let Type::Array(elem) = ty {
        // An `Array` parameter arrives in the erased `Object` slot, and
        // `arraylength` / `aaload` / `aastore` all reject a plain `Object`:
        // `g.map(_.length)` on an `Array[Array[Int]]` was a `VerifyError`
        // ("Bad type on operand stack in arraylength") although the same
        // expression outside a lambda was fine. nsc casts here too. An
        // abstract element type erases to `Object` itself, and there
        // `[Ljava/lang/Object;` would be the wrong cast (the value may be an
        // `int[]`), so leave it alone.
        if is_concrete_array_elem(elem) {
            a.checkcast(&jvm_desc(st, ty));
        }
    } else if let Type::Class { sym, .. } = ty {
        let n = class_internal(st, *sym);
        if !n.is_empty() && n != "java/lang/Object" {
            a.checkcast(&n);
        }
    } else if let Type::Tuple(ts) = ty {
        // Erasure leaves a `Type::Tuple` alone, so the arity has to be read
        // off it here. It used to be hard-coded as `Tuple2`: slick's
        // `.map(_._2)` over a `Resource[F, (Ref, CloseableIterator, …)]` cast
        // the parameter to `Tuple2` and then called `Tuple3._2` on it, which
        // the verifier threw the whole method out for.
        a.checkcast(&format!("scala/Tuple{}", ts.len().max(1)));
    } else {
        emit_unbox(a, ty);
    }
}

pub(crate) fn gen_function(asm: &mut Assembler, frame: &mut Frame, ctx: &EmitCtx, tree: &Tree) {
    let (vparams, body) = match &tree.kind {
        TreeKind::Function { vparams, body } => (vparams, body),
        _ => return,
    };
    let n = ctx.lambda_n.get();
    ctx.lambda_n.set(n + 1);
    let lam_name = format!("{}$$anonfun${}", ctx.class_name.replace('/', "$"), n);
    let arity = vparams.len();
    let is_pf = is_partial_function_ty(ctx.st, &tree.ty);
    let sam = ctx.st.sam_sig(&tree.ty);
    let iface = if is_pf {
        "scala/PartialFunction".to_string()
    } else if let Some(sam) = &sam {
        class_internal(ctx.st, sam.class)
    } else {
        format!("scala/Function{arity}")
    };

    let mut bound = HashSet::new();
    for p in vparams {
        if !p.sym.is_none() {
            bound.insert(p.sym);
        }
    }
    let mut free = FreeVars::default();
    collect_free(body, &bound, &mut free, ctx.st);

    let mut local_caps = Vec::new();
    let mut need_outer = free.uses_this;
    for id in &free.vars {
        if frame.get(*id).is_some() {
            local_caps.push(*id);
        } else {
            need_outer = true;
        }
    }
    if tree_contains_return(body) {
        need_outer = true;
    }

    // nsc 2.13 lowers a plain `FunctionN` literal to an `invokedynamic`
    // against `LambdaMetafactory` instead of a closure class; the body
    // becomes a static method of the enclosing classfile. Everything else
    // (a `PartialFunction`, a user-defined SAM type) still needs a real
    // class, so those fall through to the anonymous-class path below.
    if !is_pf && sam.is_none() && arity <= MAX_FUNCTION_ARITY {
        if let Some(owner) = ctx.hoist_owner {
            gen_function_indy(
                asm,
                frame,
                ctx,
                owner,
                n,
                &iface,
                arity,
                need_outer,
                &local_caps,
                vparams,
                body,
                &tree.ty,
            );
            return;
        }
    }

    // Read once: this is on the path of every lambda that is not hoisted, and
    // `var_os` walks the process environment on every call.
    static TRACE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *TRACE.get_or_init(|| std::env::var_os("SCALA_RS_LAMBDA_TRACE").is_some()) {
        let why = if is_pf {
            "partial-function".to_string()
        } else if let Some(s) = &sam {
            format!("sam:{}", class_internal(ctx.st, s.class))
        } else if ctx.hoist_owner.is_none() {
            "no-hoist-owner".to_string()
        } else {
            "arity".to_string()
        };
        eprintln!(
            "LAMBDA-FALLBACK {why} {} {}..{} ty={:?}",
            ctx.source, tree.span.lo.0, tree.span.hi.0, tree.ty
        );
    }

    // Create instance: new, dup, load captures, invokespecial
    asm.new_obj(&lam_name);
    asm.dup();
    let mut ctor_desc = String::from("(");
    if need_outer {
        load_this(asm, ctx);
        ctor_desc.push_str(&format!("L{};", ctx.class_name));
    }
    for id in &local_caps {
        let (slot, sort) = frame.get(*id).unwrap();
        let ty = ctx.st.get(*id).ty.clone();
        if is_boxed_var(ctx, *id) {
            // Capture the IntRef/ObjectRef itself, not the elem.
            load(asm, slot, JvmSort::Ref);
        } else {
            load(asm, slot, sort);
            if is_jvm_primitive(&ty) {
                emit_box(asm, &ty);
            }
        }
        ctor_desc.push_str("Ljava/lang/Object;");
    }
    ctor_desc.push_str(")V");
    asm.invokespecial(&lam_name, "<init>", &ctor_desc);

    // Emit the lambda class
    let mut b = ClassBuilder::new(lam_name.clone(), ctx.source);
    b.access = ACC_PUBLIC | ACC_SUPER | ACC_SYNTHETIC | ACC_FINAL;
    b.interfaces.push(iface);
    if need_outer {
        b.fields.push(Field {
            access: ACC_PUBLIC,
            name: "$outer".into(),
            desc: format!("L{};", ctx.class_name),
        });
    }
    for (i, _) in local_caps.iter().enumerate() {
        b.fields.push(Field {
            access: ACC_PUBLIC,
            name: format!("$captured${i}"),
            desc: "Ljava/lang/Object;".into(),
        });
    }
    let cap_n = local_caps.len();
    let class_name_owned = ctx.class_name.to_string();
    let need_outer_c = need_outer;
    b.add_code(
        ACC_PUBLIC,
        "<init>",
        &ctor_desc,
        1 + 1 + cap_n as u16,
        |a| {
            a.aload(0);
            a.invokespecial("java/lang/Object", "<init>", "()V");
            let mut slot = 1u16;
            if need_outer_c {
                a.aload(0);
                a.aload(slot);
                a.putfield(&lam_name, "$outer", &format!("L{class_name_owned};"));
                slot += 1;
            }
            for i in 0..cap_n {
                a.aload(0);
                a.aload(slot);
                a.putfield(&lam_name, &format!("$captured${i}"), "Ljava/lang/Object;");
                slot += 1;
            }
            a.vreturn();
        },
    );

    let mut apply_desc = String::from("(");
    for _ in 0..arity {
        apply_desc.push_str("Ljava/lang/Object;");
    }
    apply_desc.push_str(")Ljava/lang/Object;");
    let sam_emit = sam.as_ref().map(|s| {
        (
            s.name.clone(),
            jvm_method_desc(ctx.st, &s.raw_param_tys, &s.raw_ret_ty),
            s.raw_ret_ty.clone(),
        )
    });
    let (meth_name, meth_desc) = if let Some((n, d, _)) = &sam_emit {
        (n.as_str(), d.as_str())
    } else {
        ("apply", apply_desc.as_str())
    };

    let st = ctx.st;
    let extras = ctx.extras;
    let lambda_n = ctx.lambda_n;
    let lambda_bodies = ctx.lambda_bodies;
    let hoist_owner = ctx.hoist_owner;
    let source = ctx.source;
    let class_sym = ctx.class_sym;
    let library_abi = ctx.library_abi;
    let boxed = ctx.boxed_vars;
    let orig_class = ctx.class_name.to_string();
    let lam_name2 = lam_name.clone();
    let outer_desc = format!("L{orig_class};");
    let vparams = vparams.clone();
    let body = body.clone();
    let local_caps = local_caps.clone();
    let vparams_pf = vparams.clone();
    let body_pf = body.clone();
    let local_caps_pf = local_caps.clone();
    let ret_ty = if is_pf {
        body.ty.clone()
    } else if let Some(sam) = &sam {
        sam.ret_ty.clone()
    } else {
        match &tree.ty {
            Type::Function { ret, .. } => (**ret).clone(),
            t => t.clone(),
        }
    };
    let ret_ty_pf = ret_ty.clone();
    let sam_ret = sam_emit.as_ref().map(|(_, _, r)| r.clone());
    let meth_name_owned = meth_name.to_string();
    let meth_desc_owned = meth_desc.to_string();

    if is_pf {
        // nsc puts the case bodies in `applyOrElse` alone; `apply` is inherited
        // from `AbstractPartialFunction` and reads
        // `applyOrElse(x, PartialFunction.empty)`. Generating the whole `match`
        // a second time here made every lambda **class** nested inside a case
        // body come out twice, and the factor compounds with nesting: slick
        // has 130 distinct `{ case … }` literals but got 707 classfiles, one
        // literal alone appearing 128 times. `null` is the "no fallback"
        // marker `applyOrElse` turns into the `MatchError` `apply` owes.
        let lam_apply = lam_name.clone();
        b.add_code(
            ACC_PUBLIC,
            "apply",
            "(Ljava/lang/Object;)Ljava/lang/Object;",
            3,
            |a| {
                a.aload(0);
                a.aload(1);
                a.aconst_null();
                a.invokevirtual(
                    &lam_apply,
                    "applyOrElse",
                    "(Ljava/lang/Object;Lscala/Function1;)Ljava/lang/Object;",
                );
                a.areturn();
            },
        );
    } else {
        b.add_code(ACC_PUBLIC, &meth_name_owned, &meth_desc_owned, 8, |a| {
            let mut fr = Frame::instance();
            fr.next_slot = 1 + arity as u16;
            // apply args occupy slots 1..arity as Object; remap param symbols after unbox
            for (i, p) in vparams.iter().enumerate() {
                let obj_slot = 1 + i as u16;
                // A parameter instantiated at a value class receives the boxed
                // instance; erasure recorded that on the symbol.
                let p = &Tree {
                    ty: if p.sym.is_none() {
                        p.ty.clone()
                    } else {
                        st.get(p.sym).ty.clone()
                    },
                    ..p.clone()
                };
                a.aload(obj_slot);
                if is_jvm_primitive(&p.ty) || matches!(p.ty, Type::String) {
                    emit_unbox(a, &p.ty);
                } else if let Type::Array(elem) = &p.ty {
                    // An `Array` parameter arrives in the erased `Object` slot,
                    // and `arraylength` / `aaload` / `aastore` all reject a plain
                    // `Object`: `g.map(_.length)` on an `Array[Array[Int]]` was a
                    // `VerifyError` ("Bad type on operand stack in arraylength")
                    // although the same expression outside a lambda was fine. nsc
                    // casts here too. An abstract element type erases to `Object`
                    // itself, and there `[Ljava/lang/Object;` would be the wrong
                    // cast (the value may be an `int[]`), so leave it alone.
                    if is_concrete_array_elem(elem) {
                        a.checkcast(&jvm_desc(st, &p.ty));
                    }
                } else if let Type::Class { sym, .. } = &p.ty {
                    let n = class_internal(st, *sym);
                    if !n.is_empty() && n != "java/lang/Object" {
                        a.checkcast(&n);
                    }
                } else if let Type::Tuple(ts) = &p.ty {
                    a.checkcast(&format!("scala/Tuple{}", ts.len().max(1)));
                } else {
                    emit_unbox(a, &p.ty);
                }
                let sort = jvm_sort(&p.ty);
                let slot = fr.alloc(p.sym, sort);
                store(a, slot, sort);
            }
            for (i, id) in local_caps.iter().enumerate() {
                let ty = st.get(*id).ty.clone();
                a.aload(0);
                a.getfield(&lam_name2, &format!("$captured${i}"), "Ljava/lang/Object;");
                if boxed.contains(id) {
                    a.checkcast(runtime_ref_class(&ty));
                    let slot = fr.alloc(*id, JvmSort::Ref);
                    store(a, slot, JvmSort::Ref);
                } else {
                    emit_from_erased_object(a, st, &ty);
                    let sort = jvm_sort(&ty);
                    let slot = fr.alloc(*id, sort);
                    store(a, slot, sort);
                }
            }
            let outer_storage;
            let outer_ref = if need_outer {
                outer_storage = (lam_name2.as_str(), "$outer", outer_desc.as_str());
                Some(outer_storage)
            } else {
                None
            };
            let inner_ctx = EmitCtx {
                st,
                class_sym,
                class_name: &orig_class,
                ret_ty: ret_ty.clone(),
                emit_errors: std::rc::Rc::clone(&ctx.emit_errors),
                extras,
                lambda_n,
                lambda_bodies,
                hoist_owner,
                source,
                outer: outer_ref,
                presuper_outer: None,
                outer_slot: None,
                library_abi,
                method_sym: SymbolId::NONE,
                boxed_vars: boxed,
                value_ext: None,
            };
            gen_expr(a, &mut fr, &inner_ctx, &body);
            if matches!(body.ty, Type::Nothing) {
                // `throw` already emits athrow. A following areturn would be an
                // empty-stack stackmap target (`tryBreakable { throw e }`).
            } else if let Some(raw_ret) = &sam_ret {
                if is_unit_like(raw_ret) {
                    pop_if_value(a, &body.ty);
                    a.vreturn();
                } else if is_jvm_primitive(raw_ret) {
                    emit_return(a, raw_ret);
                } else {
                    if is_jvm_primitive(&ret_ty) && !is_unit_like(&ret_ty) {
                        emit_box(a, &ret_ty);
                    }
                    a.areturn();
                }
            } else if is_unit_like(&ret_ty) {
                pop_if_value(a, &body.ty);
                emit_box(a, &Type::Unit);
                a.areturn();
            } else {
                emit_box(a, &ret_ty);
                a.areturn();
            }
        });
    }
    if is_pf {
        emit_partial_function_methods(
            &mut b,
            st,
            extras,
            lambda_n,
            lambda_bodies,
            hoist_owner,
            source,
            class_sym,
            library_abi,
            &orig_class,
            &lam_name2,
            &outer_desc,
            need_outer,
            &vparams_pf,
            &body_pf,
            &local_caps_pf,
            &ret_ty_pf,
            ctx.boxed_vars,
            std::rc::Rc::clone(&ctx.emit_errors),
        );
    }
    ctx.extras.borrow_mut().push(b.finish());
}

pub(crate) fn gen_println(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    args: &[Tree],
    newline: bool,
) {
    asm.getstatic("java/lang/System", "out", "Ljava/io/PrintStream;");
    let name = if newline { "println" } else { "print" };
    if args.is_empty() {
        asm.invokevirtual("java/io/PrintStream", name, "()V");
        return;
    }
    let arg = &args[0];
    match &arg.ty.widen_constant() {
        // `println(())` prints `()`, not a blank line: nsc calls
        // `Predef.println(x: Any)` with the `BoxedUnit` singleton, whose
        // `toString` is `"()"`. The private runtime now has that class too, so
        // both modes print the same thing.
        Type::Unit | Type::NoType => {
            gen_expr(asm, frame, ctx, arg);
            if unit_leaves_boxed_ref(arg, ctx.st) {
                asm.checkcast(BOXED_UNIT);
            } else {
                emit_boxed_unit(asm);
            }
            asm.invokevirtual("java/io/PrintStream", name, "(Ljava/lang/Object;)V");
        }
        Type::Int | Type::Byte | Type::Short => {
            gen_expr(asm, frame, ctx, arg);
            asm.invokevirtual("java/io/PrintStream", name, "(I)V");
        }
        Type::Long => {
            gen_expr(asm, frame, ctx, arg);
            asm.invokevirtual("java/io/PrintStream", name, "(J)V");
        }
        Type::Double => {
            gen_expr(asm, frame, ctx, arg);
            asm.invokevirtual("java/io/PrintStream", name, "(D)V");
        }
        Type::Boolean => {
            gen_expr(asm, frame, ctx, arg);
            asm.invokevirtual("java/io/PrintStream", name, "(Z)V");
        }
        Type::String => {
            gen_expr(asm, frame, ctx, arg);
            asm.invokevirtual("java/io/PrintStream", name, "(Ljava/lang/String;)V");
        }
        Type::Char => {
            gen_expr(asm, frame, ctx, arg);
            asm.invokevirtual("java/io/PrintStream", name, "(C)V");
        }
        Type::Float => {
            gen_expr(asm, frame, ctx, arg);
            asm.invokevirtual("java/io/PrintStream", name, "(F)V");
        }
        _ => {
            gen_expr(asm, frame, ctx, arg);
            if is_jvm_primitive(&arg.ty) && !matches!(arg.ty, Type::Unit | Type::NoType) {
                emit_box(asm, &arg.ty);
            }
            asm.invokevirtual("java/io/PrintStream", name, "(Ljava/lang/Object;)V");
        }
    }
}

pub(crate) fn emit_int_bin(asm: &mut Assembler, op: &str) {
    match op {
        "+" => asm.iadd(),
        "-" => asm.isub(),
        "*" => asm.imul(),
        "/" => asm.idiv(),
        "%" => asm.irem(),
        "&" => asm.iand(),
        "|" => asm.ior(),
        "^" => asm.ixor(),
        "<<" => asm.ishl(),
        ">>" => asm.ishr(),
        ">>>" => asm.iushr(),
        "==" | "!=" | "<" | "<=" | ">" | ">=" => emit_int_cmp(asm, op),
        _ => {}
    }
}

pub(crate) fn emit_int_cmp(asm: &mut Assembler, op: &str) {
    let t = asm.fresh_label();
    let e = asm.fresh_label();
    match op {
        "==" => asm.if_icmpeq(t),
        "!=" => asm.if_icmpne(t),
        "<" => asm.if_icmplt(t),
        "<=" => asm.if_icmple(t),
        ">" => asm.if_icmpgt(t),
        ">=" => asm.if_icmpge(t),
        _ => asm.if_icmpeq(t),
    }
    asm.iconst(0);
    asm.goto(e);
    asm.mark(t);
    asm.iconst(1);
    asm.mark(e);
}

pub(crate) fn emit_ref_eq(asm: &mut Assembler, eq: bool) {
    let t = asm.fresh_label();
    let e = asm.fresh_label();
    if eq {
        asm.if_acmpeq(t);
    } else {
        asm.if_acmpne(t);
    }
    asm.iconst(0);
    asm.goto(e);
    asm.mark(t);
    asm.iconst(1);
    asm.mark(e);
}

pub(crate) fn gen_eq_ne(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    fun: &Tree,
    args: &[Tree],
    eq: bool,
) {
    gen_receiver(asm, frame, ctx, fun);
    if let Some(arg) = args.first() {
        gen_expr(asm, frame, ctx, arg);
        // A `Unit` operand is a `BoxedUnit` on the JVM but leaves nothing on
        // the stack, so the singleton has to be pushed here.
        adapt_unit_arg(asm, ctx, arg, &arg.ty);
        if is_jvm_primitive(&arg.ty) && !matches!(arg.ty, Type::Unit | Type::NoType) {
            emit_box(asm, &arg.ty);
        }
    } else {
        asm.aconst_null();
    }
    emit_ref_eq(asm, eq);
}

/// `null` written literally, however it was ascribed on the way here.
pub(crate) fn is_null_literal(t: &Tree) -> bool {
    match &t.kind {
        TreeKind::Literal { lit } => matches!(lit, Lit::Null),
        TreeKind::Typed { expr, .. } => is_null_literal(expr),
        TreeKind::Block { stats, expr } if stats.is_empty() => is_null_literal(expr),
        _ => matches!(t.ty.widen_constant(), Type::Null),
    }
}

/// A static type whose values really can be `null` on the JVM: not a
/// primitive, and not a value class (which erases to its underlying).
pub(crate) fn is_nullable_ref(st: &SymbolTable, ty: &Type) -> bool {
    let ty = ty.widen_constant();
    if is_jvm_primitive(&ty) {
        return false;
    }
    !st.class_sym_of(&ty).is_some_and(|s| st.is_value_class(s))
}

/// Leave `1` on the stack when the branch to `taken` is *not* taken, `0` when
/// it is. `emit_test` writes the conditional jump.
pub(crate) fn emit_bool_from_jump(
    asm: &mut Assembler,
    emit_test: impl FnOnce(&mut Assembler, crate::code::Label),
) {
    let is_false = asm.fresh_label();
    let end = asm.fresh_label();
    emit_test(asm, is_false);
    asm.iconst(1);
    asm.goto(end);
    asm.mark(is_false);
    asm.iconst(0);
    asm.mark(end);
}

pub(crate) fn gen_any_eq(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    fun: &Tree,
    args: &[Tree],
    eq: bool,
) {
    let recv = match &fun.kind {
        TreeKind::Select { qual, .. } => Some(&**qual),
        _ => None,
    };
    let recv_ty = recv.map(|q| q.ty.clone()).unwrap_or(Type::AnyRef);
    let arg = args.first();
    // `x == null` is a *reference* test, not a call. nsc emits a bare
    // `ifnonnull`; `x.equals(null)` threw a `NullPointerException` on exactly
    // the value the test asks about (the private runtime has no
    // `BoxesRunTime` to hide it). `null` itself has no side effect, so only
    // the other side is evaluated.
    let arg_is_null = arg.is_some_and(is_null_literal);
    let recv_is_null = recv.is_some_and(is_null_literal);
    let other = if arg_is_null {
        Some(recv_ty.clone())
    } else if recv_is_null {
        arg.map(|a| a.ty.clone())
    } else {
        None
    };
    if other.is_some_and(|t| is_nullable_ref(ctx.st, &t)) {
        if arg_is_null {
            gen_receiver(asm, frame, ctx, fun);
        } else {
            gen_expr(asm, frame, ctx, arg.unwrap());
        }
        emit_bool_from_jump(asm, |a, is_false| {
            if eq {
                a.ifnonnull(is_false)
            } else {
                a.ifnull(is_false)
            }
        });
        return;
    }
    gen_receiver(asm, frame, ctx, fun);
    if is_jvm_primitive(&recv_ty) && !is_unit_like(&recv_ty) {
        emit_box(asm, &recv_ty);
    }
    if let Some(arg) = arg {
        gen_expr(asm, frame, ctx, arg);
        // `x == ()`: the right-hand operand is a `BoxedUnit` on the JVM even
        // though the expression that produced it left nothing behind.
        adapt_unit_arg(asm, ctx, arg, &arg.ty);
        if is_jvm_primitive(&arg.ty) && !is_unit_like(&arg.ty) {
            emit_box(asm, &arg.ty);
        }
    } else {
        asm.aconst_null();
    }
    if ctx.library_abi {
        asm.invokestatic(
            "scala/runtime/BoxesRunTime",
            "equals",
            "(Ljava/lang/Object;Ljava/lang/Object;)Z",
        );
    } else if is_nullable_ref(ctx.st, &recv_ty) {
        // No `BoxesRunTime` on the private runtime, and a bare
        // `recv.equals(arg)` throws when `recv` is null. This is nsc's own
        // expansion: `if (recv == null) arg == null else recv.equals(arg)`.
        // Both sides go to locals first so every branch target has an empty
        // operand stack.
        let arg_slot = frame.alloc_tmp(JvmSort::Ref);
        store(asm, arg_slot, JvmSort::Ref);
        let recv_slot = frame.alloc_tmp(JvmSort::Ref);
        store(asm, recv_slot, JvmSort::Ref);
        let recv_null = asm.fresh_label();
        let is_true = asm.fresh_label();
        let is_false = asm.fresh_label();
        let end = asm.fresh_label();
        load(asm, recv_slot, JvmSort::Ref);
        asm.ifnull(recv_null);
        load(asm, recv_slot, JvmSort::Ref);
        load(asm, arg_slot, JvmSort::Ref);
        asm.invokevirtual("java/lang/Object", "equals", "(Ljava/lang/Object;)Z");
        asm.ifeq(is_false);
        asm.goto(is_true);
        asm.mark(recv_null);
        load(asm, arg_slot, JvmSort::Ref);
        asm.ifnonnull(is_false);
        asm.mark(is_true);
        asm.iconst(1);
        asm.goto(end);
        asm.mark(is_false);
        asm.iconst(0);
        asm.mark(end);
    } else {
        asm.invokevirtual("java/lang/Object", "equals", "(Ljava/lang/Object;)Z");
    }
    if !eq {
        asm.iconst(1);
        asm.ixor();
    }
}

pub(crate) fn gen_synchronized(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    fun: &Tree,
    args: &[Tree],
    result_ty: &Type,
) {
    gen_receiver(asm, frame, ctx, fun);
    let lock = frame.alloc_tmp(JvmSort::Ref);
    store(asm, lock, JvmSort::Ref);
    let sort = jvm_sort(result_ty);
    let result = if sort != JvmSort::Void {
        Some(frame.alloc_tmp(sort))
    } else {
        None
    };
    // Initialize the result local before the try so the exception handler
    // stack map does not claim a live integer that the body never stored.
    if let Some(r) = result {
        push_default(asm, result_ty);
        store(asm, r, sort);
    }
    load(asm, lock, JvmSort::Ref);
    asm.monitorenter();
    let try_s = asm.fresh_label();
    asm.capture_try_locals();
    // A `return` out of the body has to release the monitor first.
    let ret_exit = asm.fresh_label();
    asm.mark(try_s);
    frame.finally_exits.push(ret_exit);
    if let Some(body) = args.first() {
        let produced_ty = if let TreeKind::Function { body: inner, .. } = &body.kind {
            gen_expr(asm, frame, ctx, inner);
            inner.ty.clone()
        } else {
            gen_expr(asm, frame, ctx, body);
            if matches!(&body.ty, Type::Function { .. }) {
                asm.invokeinterface("scala/Function0", "apply", "()Ljava/lang/Object;");
            }
            body.ty.clone()
        };
        match sort {
            JvmSort::Ref => {
                if is_jvm_primitive(&produced_ty)
                    && !matches!(produced_ty, Type::Unit | Type::NoType)
                {
                    emit_box(asm, &produced_ty);
                } else if is_unit_like(&produced_ty) {
                    // Unit body: nothing (or popped) — leave a boxed null.
                    push_default(asm, result_ty);
                } else if matches!(result_ty, Type::String) && !matches!(produced_ty, Type::String)
                {
                    asm.checkcast("java/lang/String");
                }
            }
            JvmSort::Void => {
                pop_if_value(asm, &produced_ty);
            }
            _ => {
                if matches!(&produced_ty, Type::Function { .. }) {
                    emit_unbox(asm, result_ty);
                }
            }
        }
    } else {
        push_default(asm, result_ty);
    }
    if let Some(r) = result {
        store(asm, r, sort);
    }
    frame.finally_exits.pop();
    load(asm, lock, JvmSort::Ref);
    asm.monitorexit();
    let try_e = asm.fresh_label();
    asm.mark(try_e);
    let after = asm.fresh_label();
    asm.goto(after);
    let handler = asm.fresh_label();
    asm.mark(handler);
    asm.enter_handler_captured_locals();
    let ex = frame.alloc_tmp(JvmSort::Ref);
    asm.astore(ex);
    load(asm, lock, JvmSort::Ref);
    asm.monitorexit();
    asm.aload(ex);
    asm.athrow();
    asm.exception(try_s, try_e, handler, None);
    asm.release_try_locals();
    // Outside the guarded range, so a `return` does not re-enter the handler.
    asm.mark(ret_exit);
    load(asm, lock, JvmSort::Ref);
    asm.monitorexit();
    emit_pending_return(asm, frame, ctx);
    asm.mark(after);
    if let Some(r) = result {
        load(asm, r, sort);
    }
}

pub(crate) fn emit_long_bin(asm: &mut Assembler, op: &str) {
    match op {
        "+" => asm.ladd(),
        "-" => asm.lsub(),
        "*" => asm.lmul(),
        "/" => asm.ldiv(),
        "%" => asm.lrem(),
        "&" => asm.land(),
        "|" => asm.lor(),
        "^" => asm.lxor(),
        "<<" => asm.lshl(),
        ">>" => asm.lshr(),
        ">>>" => asm.lushr(),
        _ => {
            asm.lcmp();
            emit_cmp_to_bool(asm, op);
        }
    }
}

pub(crate) fn emit_double_bin(asm: &mut Assembler, op: &str) {
    match op {
        "+" => asm.dadd(),
        "-" => asm.dsub(),
        "*" => asm.dmul(),
        "/" => asm.ddiv(),
        "%" => asm.drem(),
        _ => {
            // javac's choice: `<` and `<=` use the `g` form so a NaN operand
            // makes the test false; everything else uses the `l` form.
            if matches!(op, "<" | "<=") {
                asm.dcmpg();
            } else {
                asm.dcmpl();
            }
            emit_cmp_to_bool(asm, op);
        }
    }
}

pub(crate) fn emit_float_bin(asm: &mut Assembler, op: &str) {
    match op {
        "+" => asm.fadd(),
        "-" => asm.fsub(),
        "*" => asm.fmul(),
        "/" => asm.fdiv(),
        "%" => asm.frem(),
        _ => {
            if matches!(op, "<" | "<=") {
                asm.fcmpg();
            } else {
                asm.fcmpl();
            }
            emit_cmp_to_bool(asm, op);
        }
    }
}

/// Turn the `-1 | 0 | 1` a `?cmp?` instruction leaves on the stack into the
/// boolean the comparison operator returns.
pub(crate) fn emit_cmp_to_bool(asm: &mut Assembler, op: &str) {
    let yes = asm.fresh_label();
    let end = asm.fresh_label();
    match op {
        "==" => asm.ifeq(yes),
        "!=" => asm.ifne(yes),
        "<" => asm.iflt(yes),
        "<=" => asm.ifle(yes),
        ">" => asm.ifgt(yes),
        ">=" => asm.ifge(yes),
        _ => {
            asm.pop();
            asm.iconst(0);
            return;
        }
    }
    asm.iconst(0);
    asm.goto(end);
    asm.mark(yes);
    asm.iconst(1);
    asm.mark(end);
}

pub(crate) fn gen_bool_and(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    left: &Tree,
    right: Option<&Tree>,
) {
    gen_expr(asm, frame, ctx, left);
    let skip = asm.fresh_label();
    asm.dup();
    asm.ifeq(skip);
    asm.pop();
    if let Some(r) = right {
        gen_expr(asm, frame, ctx, r);
    } else {
        asm.iconst(0);
    }
    asm.mark(skip);
}

pub(crate) fn gen_bool_or(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    left: &Tree,
    right: Option<&Tree>,
) {
    gen_expr(asm, frame, ctx, left);
    let skip = asm.fresh_label();
    asm.dup();
    asm.ifne(skip);
    asm.pop();
    if let Some(r) = right {
        gen_expr(asm, frame, ctx, r);
    } else {
        asm.iconst(1);
    }
    asm.mark(skip);
}

pub(crate) fn gen_string_concat(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    left: &Tree,
    right: &Tree,
) {
    asm.new_obj("java/lang/StringBuilder");
    asm.dup();
    asm.invokespecial("java/lang/StringBuilder", "<init>", "()V");
    gen_sb_append(asm, frame, ctx, left);
    gen_sb_append(asm, frame, ctx, right);
    asm.invokevirtual(
        "java/lang/StringBuilder",
        "toString",
        "()Ljava/lang/String;",
    );
}

pub(crate) fn gen_interpolated(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    parts: &[String],
    args: &[Tree],
) {
    asm.new_obj("java/lang/StringBuilder");
    asm.dup();
    asm.invokespecial("java/lang/StringBuilder", "<init>", "()V");
    for i in 0..args.len() {
        if i < parts.len() {
            sb_append_string(asm, &parts[i]);
        }
        gen_sb_append(asm, frame, ctx, &args[i]);
    }
    if parts.len() > args.len() {
        sb_append_string(asm, &parts[args.len()]);
    }
    asm.invokevirtual(
        "java/lang/StringBuilder",
        "toString",
        "()Ljava/lang/String;",
    );
}

pub(crate) fn gen_f_interpolated(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    parts: &[String],
    args: &[Tree],
) {
    let format = match scala_rs_parser::finterp::assemble_f(parts, args.len()) {
        Ok((fmt, _)) => fmt,
        Err(_) => {
            // Typer already diagnosed unsupported bits; keep the classfile
            // well-formed rather than inventing a successful format.
            asm.ldc_string("");
            return;
        }
    };
    asm.ldc_string(&format);
    asm.iconst(args.len() as i32);
    asm.anewarray("java/lang/Object");
    for (i, a) in args.iter().enumerate() {
        asm.dup();
        asm.iconst(i as i32);
        gen_expr(asm, frame, ctx, a);
        if is_jvm_primitive(&a.ty) {
            emit_box(asm, &a.ty);
        } else if matches!(a.ty, Type::Unit | Type::NoType) {
            // boxed already as null from emit_box
            emit_box(asm, &a.ty);
        }
        asm.aastore();
    }
    asm.invokestatic(
        "java/lang/String",
        "format",
        "(Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/String;",
    );
}

pub(crate) fn sb_append_string(asm: &mut Assembler, s: &str) {
    if s.is_empty() {
        return;
    }
    asm.ldc_string(s);
    asm.invokevirtual(
        "java/lang/StringBuilder",
        "append",
        "(Ljava/lang/String;)Ljava/lang/StringBuilder;",
    );
}

pub(crate) fn gen_sb_append(asm: &mut Assembler, frame: &mut Frame, ctx: &EmitCtx, value: &Tree) {
    let desc = match &value.ty {
        Type::Unit | Type::NoType => {
            // `s"x ${println(1)}"` still *runs* the expression; only its value
            // is the constant `"()"`. Emitting the literal alone dropped the
            // side effect, so the interpolation printed `x ()` and the `1`
            // never appeared. `gen_stat` is the statement form: it discards
            // whatever the call really leaves on the stack.
            gen_stat(asm, frame, ctx, value);
            asm.ldc_string("()");
            "(Ljava/lang/String;)Ljava/lang/StringBuilder;"
        }
        Type::Int | Type::Byte | Type::Short => {
            gen_expr(asm, frame, ctx, value);
            "(I)Ljava/lang/StringBuilder;"
        }
        Type::Long => {
            gen_expr(asm, frame, ctx, value);
            "(J)Ljava/lang/StringBuilder;"
        }
        Type::Double => {
            gen_expr(asm, frame, ctx, value);
            "(D)Ljava/lang/StringBuilder;"
        }
        Type::Float => {
            gen_expr(asm, frame, ctx, value);
            "(F)Ljava/lang/StringBuilder;"
        }
        Type::Boolean => {
            gen_expr(asm, frame, ctx, value);
            "(Z)Ljava/lang/StringBuilder;"
        }
        Type::Char => {
            gen_expr(asm, frame, ctx, value);
            "(C)Ljava/lang/StringBuilder;"
        }
        Type::String => {
            gen_expr(asm, frame, ctx, value);
            "(Ljava/lang/String;)Ljava/lang/StringBuilder;"
        }
        _ => {
            gen_expr(asm, frame, ctx, value);
            "(Ljava/lang/Object;)Ljava/lang/StringBuilder;"
        }
    };
    asm.invokevirtual("java/lang/StringBuilder", "append", desc);
}

/// Park every value already on the operand stack in a local, so the guarded
/// region of a `try` starts with an empty stack.
///
/// The JVM clears the operand stack when it enters an exception handler
/// (JVMS 4.10.1.6), so anything pending is gone on the catch path: the join
/// after the `try` then sees a stack of depth *n* on one side and 0 on the
/// other, and the frames disagree -- `VerifyError: Inconsistent stackmap
/// frames`. `println(try … catch …)`, `f(a, try …)` and `new Box(try …)` all
/// hit it. scalac's `LiftTry` phase moves the `try` into a synthetic
/// `liftedTree1$1()` method for exactly this reason; spilling to locals is the
/// same fix without the extra method.
///
/// Returns the spill slots bottom-first, for [`restore_operand_stack`].
pub(crate) fn spill_operand_stack(asm: &mut Assembler, frame: &mut Frame) -> Vec<(u16, JvmSort)> {
    let entries = asm.stack_entries();
    if entries.is_empty() {
        return Vec::new();
    }
    let mut spilled = Vec::with_capacity(entries.len());
    // Top of the stack first: a store pops from the top.
    for e in entries.iter().rev() {
        let sort = match e {
            StackEntry::Int => JvmSort::Int,
            StackEntry::Long => JvmSort::Long,
            StackEntry::Float => JvmSort::Float,
            StackEntry::Double => JvmSort::Double,
            StackEntry::Ref(_) => JvmSort::Ref,
        };
        let slot = frame.alloc_tmp(sort);
        // `None` is `null` or a `new` whose constructor has not run yet;
        // neither has a class to declare, and the uninitialized one must keep
        // its own verification type so the later `invokespecial` still matches.
        if let StackEntry::Ref(Some(n)) = e {
            asm.set_local_class(slot, n);
        }
        store(asm, slot, sort);
        spilled.push((slot, sort));
    }
    spilled.reverse();
    spilled
}

pub(crate) fn restore_operand_stack(asm: &mut Assembler, spilled: &[(u16, JvmSort)]) {
    for (slot, sort) in spilled {
        load(asm, *slot, *sort);
    }
}

pub(crate) fn gen_try(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    block: &Tree,
    catches: &[scala_rs_parser::CaseDef],
    finalizer: &Tree,
    result_ty: &Type,
) {
    // Before anything else, and before the locals snapshot the handlers use:
    // the guarded region has to start with an empty operand stack.
    let spilled = spill_operand_stack(asm, frame);
    let unit = is_unit_like(result_ty);
    let has_finally = !finalizer.is_empty();
    let start = asm.fresh_label();
    let end_try = asm.fresh_label();
    let handler = asm.fresh_label();
    let after = asm.fresh_label();
    let sel_sort = if unit {
        JvmSort::Void
    } else {
        jvm_sort(result_ty)
    };
    let result_slot = if unit {
        None
    } else {
        Some(frame.alloc_tmp(sel_sort))
    };
    // Initialize the result local before the try so the exception handler
    // stack map does not claim a live value that the body never stored.
    if let Some(slot) = result_slot {
        // The body and every catch store their own class into this slot; the
        // static type of the whole `try` is the class it is declared to hold.
        if let Some(n) = join_class_of(ctx.st, result_ty) {
            asm.set_local_class(slot, &n);
        }
        push_default(asm, result_ty);
        store(asm, slot, sel_sort);
    }
    let exn_slot = frame.alloc_tmp(JvmSort::Ref);

    // The handlers below can be entered from any point of the guarded region, so
    // their frames may only mention the locals that are live before it starts.
    asm.capture_try_locals();
    // A `return` out of the guarded body must not skip the finalizer; it jumps
    // to `ret_exit`, which runs it (and any enclosing ones) and then returns.
    let ret_exit = if has_finally {
        Some(asm.fresh_label())
    } else {
        None
    };
    asm.mark(start);
    if let Some(l) = ret_exit {
        frame.finally_exits.push(l);
    }
    if unit {
        gen_stat(asm, frame, ctx, block);
    } else {
        gen_expr(asm, frame, ctx, block);
        if matches!(block.ty, Type::Nothing) {
            // `val n = try throw e catch h`: the body always throws, so it
            // leaves nothing behind. The store is unreachable, but the
            // verifier still needs a value of the right sort under it.
            push_default(asm, result_ty);
        }
        box_for_result_slot(asm, &block.ty, sel_sort);
        store(asm, result_slot.unwrap(), sel_sort);
    }
    if ret_exit.is_some() {
        frame.finally_exits.pop();
    }
    asm.mark(end_try);
    if has_finally {
        gen_stat(asm, frame, ctx, finalizer);
    }
    asm.goto(after);

    asm.mark(handler);
    asm.enter_handler_captured_locals();
    store(asm, exn_slot, JvmSort::Ref);
    let catch_rethrow = if has_finally && !catches.is_empty() {
        Some(asm.fresh_label())
    } else {
        None
    };
    for c in catches {
        let fail = asm.fresh_label();
        gen_pattern(asm, frame, ctx, &c.pat, exn_slot, JvmSort::Ref, fail);
        if !c.guard.is_empty() {
            gen_expr(asm, frame, ctx, &c.guard);
            asm.ifeq(fail);
        }
        let catch_start = asm.fresh_label();
        let catch_end = asm.fresh_label();
        asm.mark(catch_start);
        if let Some(l) = ret_exit {
            frame.finally_exits.push(l);
        }
        if unit {
            gen_stat(asm, frame, ctx, &c.body);
        } else if is_unit_like(&c.body.ty) {
            // `try { resources(...) /* Int */ } catch { println }` — nsc LUBs
            // to Any in statement position. We keep the try's type and fill
            // a default so the catch does not istore from an empty stack.
            gen_stat(asm, frame, ctx, &c.body);
            push_default(asm, result_ty);
            if let Some(slot) = result_slot {
                store(asm, slot, sel_sort);
            }
        } else {
            gen_expr(asm, frame, ctx, &c.body);
            box_for_result_slot(asm, &c.body.ty, sel_sort);
            if let Some(slot) = result_slot {
                store(asm, slot, sel_sort);
            }
        }
        if ret_exit.is_some() {
            frame.finally_exits.pop();
        }
        asm.mark(catch_end);
        if has_finally {
            gen_stat(asm, frame, ctx, finalizer);
        }
        asm.goto(after);
        if let Some(rethrow) = catch_rethrow {
            asm.exception(catch_start, catch_end, rethrow, Some("java/lang/Throwable"));
        }
        asm.mark(fail);
    }
    if has_finally {
        gen_stat(asm, frame, ctx, finalizer);
    }
    load(asm, exn_slot, JvmSort::Ref);
    asm.athrow();

    if let Some(rethrow) = catch_rethrow {
        asm.mark(rethrow);
        asm.enter_handler_captured_locals();
        store(asm, exn_slot, JvmSort::Ref);
        gen_stat(asm, frame, ctx, finalizer);
        load(asm, exn_slot, JvmSort::Ref);
        asm.athrow();
    }
    asm.release_try_locals();

    if let Some(l) = ret_exit {
        // Outside every guarded range: a finalizer that throws here must not be
        // caught by the handler that would run it a second time.
        asm.mark(l);
        gen_stat(asm, frame, ctx, finalizer);
        emit_pending_return(asm, frame, ctx);
    }

    asm.mark(after);
    // Put back what was pending before the `try`, under its result.
    restore_operand_stack(asm, &spilled);
    if let Some(slot) = result_slot {
        load(asm, slot, sel_sort);
    }
    asm.exception(start, end_try, handler, Some("java/lang/Throwable"));
}

/// A `try` parks its result in one local of one sort, but its branches have
/// their own types: `try n catch { case _: Exception => "x" }` is an `Any`
/// whose body leaves an `int`. Box it when the slot wants a reference and the
/// branch did not already box (the assembler knows which, the tree's type does
/// not -- an adaptation may have boxed it on the way).
pub(crate) fn box_for_result_slot(asm: &mut Assembler, ty: &Type, sel_sort: JvmSort) {
    if sel_sort != JvmSort::Ref || asm.top_is_reference() {
        return;
    }
    if matches!(ty, Type::Nothing) {
        return;
    }
    emit_box(asm, ty);
}

/// The class a value of `ty` has on the JVM, for the frame that merges the
/// branches of a `match` or an `if`.
///
/// The branches push different classes (`scala/Some` and `scala/None$`), and
/// the assembler has no class hierarchy to take their least upper bound with.
/// The expression's own static type *is* an upper bound of all of them, so
/// hand that over instead. `None` where there is nothing to say: a primitive,
/// `Object` itself, or a branchless `Unit`/`Nothing`.
pub(crate) fn join_class_of(st: &SymbolTable, ty: &Type) -> Option<String> {
    if is_unit_like(ty) || matches!(ty, Type::Nothing | Type::NoType | Type::Error) {
        return None;
    }
    let desc = jvm_desc(st, ty);
    if desc.starts_with('[') {
        return Some(desc);
    }
    let name = desc.strip_prefix('L')?.strip_suffix(';')?;
    if name == "java/lang/Object" {
        return None;
    }
    Some(name.to_string())
}

/// Declare what class the local `slot` is *declared* to hold, so a merge at a
/// loop head or a branch join reports the declared class instead of falling
/// back to `java/lang/Object`.
///
/// This is what scalac does: `javap -v` on
/// `var c: Option[Int] = Some(1); while (c.isDefined) c = None` shows the
/// loop-head frame carrying `class scala/Option` -- the *declared* erased type
/// of the slot, the same type its `LocalVariableTable` entry has -- not a
/// computed least upper bound of `scala/Some` and `scala/None$`. The declared
/// type is by construction an upper bound of everything the source can store
/// there, so recording it needs no class hierarchy and never widens too far.
pub(crate) fn declare_local_ty(asm: &mut Assembler, st: &SymbolTable, slot: u16, ty: &Type) {
    if let Some(n) = local_class_of(st, ty) {
        asm.set_local_class(slot, &n);
    }
}

/// The erased class a local of type `ty` is declared to hold, as an internal
/// name (or an array descriptor).
///
/// Unlike [`join_class_of`] this keeps `java/lang/Object`: `var a: Any` really
/// is declared `Object`, and saying so is what keeps the frames inside a loop
/// that stores an `Integer` and then a `String` into it consistent.
pub(crate) fn local_class_of(st: &SymbolTable, ty: &Type) -> Option<String> {
    if is_unit_like(ty) || matches!(ty, Type::Nothing | Type::NoType | Type::Error) {
        return None;
    }
    let desc = jvm_desc(st, ty);
    if desc.starts_with('[') {
        return Some(desc);
    }
    Some(desc.strip_prefix('L')?.strip_suffix(';')?.to_string())
}
