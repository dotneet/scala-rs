//! Direct self tail calls become backwards branches. Selection is structural:
//! arguments, guards, nested definitions and try/finally are never tail positions.
use crate::code::{Assembler, Label};
use crate::gen::*;
use scala_rs_parser::{Flags, SymbolId, Tree, TreeKind, Type};
use scala_rs_typer::SymKind;
use std::collections::HashSet;

pub(crate) struct TailLoop {
    head: Label,
    calls: HashSet<usize>,
    params: Vec<(u16, JvmSort)>,
    types: Vec<Type>,
    pending: HashSet<usize>,
    annotated: bool,
}

fn call(tree: &Tree, method: SymbolId, nullary: bool) -> Option<(&Tree, Vec<Tree>)> {
    let (fun, args) = match &tree.kind {
        TreeKind::Apply { fun, args } => flatten_apply_owned(fun, args),
        TreeKind::Select { .. } | TreeKind::Ident { .. } if nullary => (tree, vec![]),
        _ => return None,
    };
    (fun.sym == method && matches!(fun.kind, TreeKind::Ident { .. } | TreeKind::Select { .. }))
        .then_some((fun, args))
}

fn collect(tree: &Tree, method: SymbolId, nullary: bool, calls: &mut HashSet<usize>) {
    if call(tree, method, nullary).is_some() {
        calls.insert(tree as *const Tree as usize);
        return;
    }
    match &tree.kind {
        TreeKind::If { thenp, elsep, .. } => {
            collect(thenp, method, nullary, calls);
            collect(elsep, method, nullary, calls);
        }
        TreeKind::Block { expr, .. } | TreeKind::Typed { expr, .. } => {
            collect(expr, method, nullary, calls);
        }
        TreeKind::Match { cases, .. } => {
            for c in cases {
                collect(&c.body, method, nullary, calls);
            }
        }
        _ => {}
    }
}

pub(crate) fn begin_tail_loop(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    rhs: &Tree,
) -> Option<String> {
    let method = ctx.method_sym;
    if method.is_none() {
        return None;
    }
    let s = ctx.st.get(method);
    let owner = ctx.st.get(s.owner);
    // Annotated methods have already passed the typer's more complete
    // effectively-final check (including sealed classes without overrides).
    let annotated = s
        .annotations
        .iter()
        .any(|a| a.annotation_path().rsplit('.').next() == Some("tailrec"));
    let eligible = annotated
        || s.flags.contains(Flags::FINAL)
        || s.flags.contains(Flags::PRIVATE)
        || owner.flags.contains(Flags::FINAL)
        || matches!(
            owner.kind,
            SymKind::Module | SymKind::ModuleClass | SymKind::Method
        );
    if !eligible || s.name == "<init>" {
        return None;
    }
    let unsupported = || {
        annotated.then(|| format!("could not optimize @tailrec annotated method {}: unsupported erased tail-call shape", s.name))
    };
    // A source value-class instance method is still emitted as a boxed helper
    // beside the real `$extension` static. Its erased self calls are already
    // routed through the extension path, so only the static body has a slot 0
    // underlying receiver that this loop transform can update safely.
    if ctx.outer.is_some() {
        return unsupported();
    }
    // The boxed helper emitted beside a source value class delegates to its
    // `$extension` static. Leave that helper's body in its ordinary form; the
    // static below is the method whose slot 0 can be rewritten safely.
    if ctx.value_ext.is_none() && ctx.st.is_value_class(ctx.class_sym) {
        return None;
    }
    let mut calls = HashSet::new();
    collect(rhs, method, s.paramss.is_empty(), &mut calls);
    if calls.is_empty() {
        return unsupported();
    }
    let Some(params) = s
        .params
        .iter()
        .map(|id| frame.get(*id))
        .collect::<Option<Vec<_>>>()
    else {
        return unsupported();
    };
    let types = method_params_from_sym(ctx.st, method);
    if params.len() != types.len() {
        return unsupported();
    }
    // Tail-call arguments are emitted through the erased call path.  A
    // constructor or factory used as the next value can therefore be tracked
    // as `java/lang/Object` even when the parameter's descriptor is a narrower
    // reference (for example `Option` receiving `Some`).  Declare each
    // recursive parameter slot before recording the loop head so stores on the
    // back edge retain the method's erased parameter class in StackMapTable
    // frames.  Without this, the first iteration enters a branch with the
    // descriptor type while the back edge reaches it as Object, which the JVM
    // verifier rejects.
    for ((slot, _), ty) in params.iter().zip(&types) {
        declare_local_ty(asm, ctx.st, *slot, ty);
    }
    let head = asm.fresh_label();
    asm.mark(head);
    frame.tail_loop = Some(TailLoop {
        head,
        pending: calls.clone(),
        calls,
        params,
        types,
        annotated,
    });
    None
}

pub(crate) fn emit_tail_call(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    tree: &Tree,
) -> bool {
    let Some(lp) = &frame.tail_loop else {
        return false;
    };
    if !lp.calls.contains(&(tree as *const Tree as usize)) {
        return false;
    }
    let head = lp.head;
    let params = lp.params.clone();
    let types = lp.types.clone();
    let Some((fun, args)) = call(
        tree,
        ctx.method_sym,
        ctx.st.get(ctx.method_sym).paramss.is_empty(),
    ) else {
        return false;
    };
    if args.len() != params.len() {
        return false;
    }
    // Evaluate the receiver before arguments and keep it below them until
    // all argument assignments finish. Like nsc TailCalls, rebind `this`
    // without an extra null check; this deliberately follows the observable
    // nsc behaviour even when a null receiver is not dereferenced in the body.
    let receiver = match &fun.kind {
        TreeKind::Select { qual, .. } if !matches!(qual.kind, TreeKind::This { .. }) => Some(qual),
        _ => None,
    };
    if let Some(receiver) = receiver {
        gen_expr(asm, frame, ctx, receiver);
    }
    gen_call_args(asm, frame, ctx, &args, &types, true, false, ctx.method_sym);
    for ((slot, sort), ty) in params.iter().zip(&types).rev() {
        if is_unit_like(ty) {
            asm.pop();
        } else {
            store(asm, *slot, *sort);
        }
    }
    if receiver.is_some() {
        // A value-class extension keeps its underlying receiver in slot 0.
        // The receiver expression may itself be a value-class constructor, so
        // its sort is the underlying primitive (including the two-slot Long /
        // Double cases), rather than the boxed class reference used by an
        // ordinary instance method.
        let receiver_sort = ctx
            .value_ext
            .as_ref()
            .map(|(_, _, sort)| *sort)
            .unwrap_or(JvmSort::Ref);
        store(asm, 0, receiver_sort);
    }
    frame
        .tail_loop
        .as_mut()
        .unwrap()
        .pending
        .remove(&(tree as *const Tree as usize));
    asm.goto(head);
    true
}

/// Never report successful compilation of an annotation whose accepted
/// recursive calls were lost by a later lowering or unsupported call shape.
pub(crate) fn finish_tail_loop(frame: &Frame, ctx: &EmitCtx) -> Option<String> {
    frame.tail_loop.as_ref().filter(|lp| lp.annotated && !lp.pending.is_empty()).map(|_| {
        format!("could not optimize @tailrec annotated method {}: unsupported erased tail-call shape", ctx.st.get(ctx.method_sym).name)
    })
}
