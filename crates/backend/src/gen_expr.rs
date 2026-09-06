//! Statement and expression code generation: returns (plain, non-local and
//! lazy-cell), literals, `this`, identifiers, `Select`, assignment, `if`,
//! `new`, and the `Apply` dispatcher together with the receiver and value
//! class `$extension` paths it delegates to.

use crate::classfile::encode_method_name;
use crate::code::Assembler;
use crate::companion_fwd::DescSort;
use crate::gen::*;
use scala_rs_parser::{Flags, Lit, SymbolId, Tree, TreeKind, Type};
use scala_rs_typer::{Intrinsic, SymKind, SymbolTable};

pub(crate) fn jvm_sort_of(sort: DescSort) -> JvmSort {
    match sort {
        DescSort::Int => JvmSort::Int,
        DescSort::Long => JvmSort::Long,
        DescSort::Float => JvmSort::Float,
        DescSort::Double => JvmSort::Double,
        DescSort::Ref => JvmSort::Ref,
        DescSort::Void => JvmSort::Void,
    }
}

pub(crate) fn load(asm: &mut Assembler, slot: u16, sort: JvmSort) {
    match sort {
        JvmSort::Int => asm.iload(slot),
        JvmSort::Long => asm.lload(slot),
        JvmSort::Double => asm.dload(slot),
        JvmSort::Float => asm.fload(slot),
        JvmSort::Ref => asm.aload(slot),
        JvmSort::Void => {}
    }
}

pub(crate) fn store(asm: &mut Assembler, slot: u16, sort: JvmSort) {
    match sort {
        JvmSort::Int => asm.istore(slot),
        JvmSort::Long => asm.lstore(slot),
        JvmSort::Double => asm.dstore(slot),
        JvmSort::Float => asm.fstore(slot),
        JvmSort::Ref => asm.astore(slot),
        JvmSort::Void => {}
    }
}

pub(crate) fn pop_if_value(asm: &mut Assembler, ty: &Type) {
    pop_sort(asm, jvm_sort(ty));
}

pub(crate) fn pop_sort(asm: &mut Assembler, sort: JvmSort) {
    match sort {
        JvmSort::Void => {}
        JvmSort::Long | JvmSort::Double => asm.pop2(),
        _ => asm.pop(),
    }
}

/// Leave the method with the value parked by a `return` that had finalizers to
/// run: hand it to the next enclosing finalizer, or return it here.
pub(crate) fn emit_pending_return(asm: &mut Assembler, frame: &mut Frame, ctx: &EmitCtx) {
    if let Some(outer) = frame.finally_exits.last().copied() {
        asm.goto(outer);
        return;
    }
    if !is_unit_like(&ctx.ret_ty) {
        let sort = jvm_sort(&ctx.ret_ty);
        match frame.return_slot {
            Some(slot) => load(asm, slot, sort),
            // No `return` reached this exit, so the block is unreachable and
            // will be dropped; keep the stack consistent all the same.
            None => push_default(asm, &ctx.ret_ty),
        }
    }
    emit_return(asm, &ctx.ret_ty);
}

pub(crate) fn emit_return(asm: &mut Assembler, ty: &Type) {
    if matches!(ty, Type::Nothing) {
        // `jvm_sort(Nothing) == Void`, right for a value slot's *stack
        // effect* (an expression of type `Nothing` never actually leaves a
        // value behind for the rest of codegen to track), but `jvm_desc`
        // declares such a method's result `Lscala/runtime/Nothing$;` -- a
        // reference, never `V` -- matching nsc's own erasure. A live return
        // through this type (a static forwarder to a module method declared
        // `Nothing`, a by-name lambda whose SAM result is `Nothing`) really
        // does have that reference sitting on the stack and has to hand it
        // back with `areturn`, exactly what `javap -c` shows nsc emitting
        // there (confirmed on `T1.die()` 's static forwarder). Everywhere
        // else this call is dead code padding after `gen_expr`'s own
        // `athrow`, where the opcode choice cannot matter.
        asm.areturn();
        return;
    }
    ret_of_sort(asm, jvm_sort(ty));
}

/// A bridge or forwarder has just called a method whose *result descriptor* is
/// `target_ret`. When that is `scala/runtime/Nothing$` the value cannot be
/// handed on to anything: `Nothing$` is a subtype of nothing at all, so
/// `areturn`ing it out of a method declared to return something else is
/// `VerifyError: Bad return type`, and passing it as an argument is the same
/// error one frame down. The call cannot complete normally in the first place,
/// so nsc's `BCodeBodyBuilder.adapt` follows it with `athrow` and lets the
/// verifier stop looking. Confirmed with `javap -c` on scalac 2.13.16 for both
/// shapes slick has:
///
/// ```text
/// public java.lang.String compiler();       // override lazy val compiler: Nothing = …
///   invokevirtual compiler:()Lscala/runtime/Nothing$;
///   athrow
/// public void update(java.lang.Object, java.lang.Object);
///   … checkcast scala/runtime/Nothing$
///   invokevirtual update:(Ljava/lang/Object;Lscala/runtime/Nothing$;)Lscala/runtime/Nothing$;
///   athrow
/// ```
///
/// [`gen_expr`] applies this rule to every *expression* of type `Nothing`;
/// bridges are hand-assembled and never build a `Tree`, so they need it here.
/// Returns `true` when it emitted the `athrow`, so the caller skips its own
/// adapt-and-return.
pub(crate) fn emit_forwarded_nothing(asm: &mut Assembler, target_ret: &str) -> bool {
    if target_ret == NOTHING_DESC {
        asm.athrow();
        return true;
    }
    false
}

pub(crate) fn ret_of_sort(asm: &mut Assembler, sort: JvmSort) {
    match sort {
        JvmSort::Void => asm.vreturn(),
        JvmSort::Int => asm.ireturn(),
        JvmSort::Long => asm.lreturn(),
        JvmSort::Double => asm.dreturn(),
        JvmSort::Float => asm.freturn(),
        JvmSort::Ref => asm.areturn(),
    }
}

pub(crate) const NLRC: &str = "scala/runtime/NonLocalReturnControl";

pub(crate) fn emit_nonlocal_return(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    expr: &Tree,
) {
    asm.new_obj(NLRC);
    asm.dup();
    load_this(asm, ctx);
    if !expr.is_empty() && !is_unit_like(&expr.ty) {
        gen_expr(asm, frame, ctx, expr);
        emit_box(asm, &expr.ty);
    } else {
        if !expr.is_empty() {
            gen_expr(asm, frame, ctx, expr);
            pop_if_value(asm, &expr.ty);
        }
        asm.aconst_null();
    }
    asm.invokespecial(NLRC, "<init>", "(Ljava/lang/Object;Ljava/lang/Object;)V");
    asm.athrow();
}

/// The element descriptor a `scala.runtime.Lazy…` cell stores, or `None` for
/// `LazyUnit`, which keeps only the flag.
pub(crate) fn lazy_cell_elem(internal: &str) -> Option<&'static str> {
    match internal.rsplit('/').next().unwrap_or("") {
        "LazyBoolean" => Some("Z"),
        "LazyByte" => Some("B"),
        "LazyChar" => Some("C"),
        "LazyShort" => Some("S"),
        "LazyInt" => Some("I"),
        "LazyLong" => Some("J"),
        "LazyFloat" => Some("F"),
        "LazyDouble" => Some("D"),
        "LazyUnit" => None,
        _ => Some("Ljava/lang/Object;"),
    }
}

/// Bring a value read out of an `Ljava/lang/Object;` cell back to `ret`.
/// Only `LazyRef` needs this; the unboxed cells already hold the right sort.
pub(crate) fn lazy_cell_from_object(asm: &mut Assembler, ctx: &EmitCtx, ret: &Type) {
    if is_jvm_primitive(ret) && !is_unit_like(ret) {
        emit_unbox(asm, ret);
        return;
    }
    if matches!(ret, Type::String) {
        asm.checkcast("java/lang/String");
        return;
    }
    if let Type::Array(elem) = ret {
        if is_concrete_array_elem(elem) {
            let d = jvm_desc(ctx.st, ret);
            asm.checkcast(&d);
        }
        return;
    }
    if let Some(cn) = checkcast_internal(ctx.st, ret) {
        if cn != "java/lang/Object" && has_class_sym(ctx.st, ret) {
            asm.checkcast(&cn);
        }
    }
}

/// `cell.value()`, coerced to `ret`. Pushes nothing for `LazyUnit`.
pub(crate) fn emit_lazy_cell_read(
    asm: &mut Assembler,
    ctx: &EmitCtx,
    cn: &str,
    elem: Option<&str>,
    slot: u16,
    ret: &Type,
) {
    let Some(e) = elem else {
        return;
    };
    asm.aload(slot);
    asm.invokevirtual(cn, "value", &format!("(){e}"));
    if e == "Ljava/lang/Object;" {
        lazy_cell_from_object(asm, ctx, ret);
    }
}

/// nsc's `lazyvals` shape for a method-local `lazy val`, one method instead of
/// scalac's `x$1` + `x$lzycompute$1` pair:
///
/// ```text
/// if (cell.initialized()) cell.value()
/// else synchronized (cell) {
///   if (cell.initialized()) cell.value() else cell.initialize(<rhs>)
/// }
/// ```
///
/// `_initialized` is only set *after* the value is stored, so an initialiser
/// that throws leaves the cell untouched and the next read retries it — the
/// same as scalac.
pub(crate) fn emit_local_lazy_body(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    rhs: &Tree,
    ret: &Type,
    cell: SymbolId,
) {
    let Some((slot, _)) = frame.get(cell) else {
        throw_runtime(asm, "lazy val cell is missing");
        push_default(asm, ret);
        emit_return(asm, ret);
        return;
    };
    let cell_ty = ctx.st.get(cell).ty.clone();
    let cn = class_internal(
        ctx.st,
        ctx.st.class_sym_of(&cell_ty).unwrap_or(SymbolId::NONE),
    );
    let elem = lazy_cell_elem(&cn);
    let sort = jvm_sort(ret);

    // Fast path: no monitor once the cell is initialised.
    asm.aload(slot);
    asm.invokevirtual(&cn, "initialized", "()Z");
    let slow = asm.fresh_label();
    asm.ifeq(slow);
    emit_lazy_cell_read(asm, ctx, &cn, elem, slot, ret);
    emit_return(asm, ret);
    asm.mark(slow);

    let lock = frame.alloc_tmp(JvmSort::Ref);
    asm.aload(slot);
    store(asm, lock, JvmSort::Ref);
    let result = if sort != JvmSort::Void {
        Some(frame.alloc_tmp(sort))
    } else {
        None
    };
    // Stored before the guarded range so the handler's stack map does not
    // claim a local the body has not written yet.
    if let Some(r) = result {
        push_default(asm, ret);
        store(asm, r, sort);
    }
    load(asm, lock, JvmSort::Ref);
    asm.monitorenter();
    asm.capture_try_locals();
    let try_s = asm.fresh_label();
    asm.mark(try_s);

    asm.aload(slot);
    asm.invokevirtual(&cn, "initialized", "()Z");
    let compute = asm.fresh_label();
    asm.ifeq(compute);
    emit_lazy_cell_read(asm, ctx, &cn, elem, slot, ret);
    let done = asm.fresh_label();
    asm.goto(done);
    asm.mark(compute);
    match elem {
        None => {
            // `LazyUnit`: there is no value to keep, only the flag.
            gen_expr(asm, frame, ctx, rhs);
            pop_if_value(asm, &rhs.ty);
            asm.aload(slot);
            asm.invokevirtual(&cn, "initialize", "()V");
        }
        Some(e) => {
            asm.aload(slot);
            gen_expr(asm, frame, ctx, rhs);
            if e == "Ljava/lang/Object;" {
                if is_jvm_primitive(&rhs.ty) && !is_unit_like(&rhs.ty) {
                    emit_box(asm, &rhs.ty);
                } else if is_unit_like(&rhs.ty) {
                    push_default(asm, ret);
                }
            }
            asm.invokevirtual(&cn, "initialize", &format!("({e}){e}"));
            if e == "Ljava/lang/Object;" {
                lazy_cell_from_object(asm, ctx, ret);
            }
        }
    }
    asm.mark(done);
    if let Some(r) = result {
        store(asm, r, sort);
    }
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
    asm.mark(after);
    if let Some(r) = result {
        load(asm, r, sort);
    }
    emit_return(asm, ret);
}

pub(crate) fn finish_method_body(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    rhs: &Tree,
    ret: &Type,
) {
    // A method-local `lazy val`'s accessor: `rhs` is the initialiser, and it
    // runs at most once, behind the cell this method was handed.
    if !ctx.method_sym.is_none() {
        if let Some(&cell) = ctx.st.local_lazy_accessors.get(&ctx.method_sym) {
            emit_local_lazy_body(asm, frame, ctx, rhs, ret, cell);
            return;
        }
    }
    let wrap = !ctx.method_sym.is_none()
        && (tree_has_nlr_to(rhs, ctx.method_sym)
            // A `return` that moved into a hoisted local-`lazy val` accessor is
            // no longer visible in this method's own body.
            || ctx.st.local_lazy_nlr.contains(&ctx.method_sym));
    if wrap {
        asm.capture_try_locals();
        let start = asm.fresh_label();
        let end = asm.fresh_label();
        let handler = asm.fresh_label();
        asm.mark(start);
        emit_body_return(asm, frame, ctx, rhs, ret);
        asm.mark(end);
        asm.mark(handler);
        asm.enter_handler_captured_locals();
        asm.checkcast(NLRC);
        asm.dup();
        asm.invokevirtual(NLRC, "key", "()Ljava/lang/Object;");
        asm.aload(0);
        let rethrow = asm.fresh_label();
        asm.if_acmpne(rethrow);
        asm.invokevirtual(NLRC, "value", "()Ljava/lang/Object;");
        if is_unit_like(ret) {
            asm.pop();
            asm.vreturn();
        } else {
            // `NonLocalReturnControl.value()` is declared `()Ljava/lang/Object;`,
            // so a reference result needs the `checkcast` the method's own
            // descriptor promises -- without it `def f[T](…): Option[T]` with a
            // `return` inside a `foreach` lambda ended in `areturn` on an
            // `Object` and the verifier rejected it ("Bad return type ... is
            // not assignable to 'scala/Option' (from method signature)").
            // `lazy_cell_from_object` is the same Object -> `ret` coercion the
            // lazy-cell and `Breaks` readers use: unbox primitives, cast the
            // reference types that have runtime class information, leave the
            // erased ones alone.
            lazy_cell_from_object(asm, ctx, ret);
            emit_return(asm, ret);
        }
        asm.mark(rethrow);
        asm.athrow();
        asm.exception(start, end, handler, Some(NLRC));
        asm.release_try_locals();
    } else {
        emit_body_return(asm, frame, ctx, rhs, ret);
    }
}

pub(crate) fn emit_body_return(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    rhs: &Tree,
    ret: &Type,
) {
    gen_expr(asm, frame, ctx, rhs);
    if is_unit_like(ret) {
        pop_if_value(asm, &rhs.ty);
        asm.vreturn();
    } else {
        emit_return(asm, ret);
    }
}

pub(crate) fn tree_contains_return(tree: &Tree) -> bool {
    match &tree.kind {
        TreeKind::Return { .. } => true,
        TreeKind::Select { qual, .. } => tree_contains_return(qual),
        TreeKind::Apply { fun, args } | TreeKind::UnApply { fun, args } => {
            tree_contains_return(fun) || args.iter().any(tree_contains_return)
        }
        TreeKind::TypeApply { fun, .. } | TreeKind::Typed { expr: fun, .. } => {
            tree_contains_return(fun)
        }
        TreeKind::Block { stats, expr } => {
            stats.iter().any(tree_contains_return) || tree_contains_return(expr)
        }
        TreeKind::If { cond, thenp, elsep } => {
            tree_contains_return(cond) || tree_contains_return(thenp) || tree_contains_return(elsep)
        }
        TreeKind::Assign { lhs, rhs } => tree_contains_return(lhs) || tree_contains_return(rhs),
        TreeKind::ValDef { rhs, .. } => tree_contains_return(rhs),
        TreeKind::Function { body, vparams } => {
            vparams.iter().any(tree_contains_return) || tree_contains_return(body)
        }
        TreeKind::Match { selector, cases } => {
            tree_contains_return(selector)
                || cases.iter().any(|c| {
                    tree_contains_return(&c.pat)
                        || tree_contains_return(&c.guard)
                        || tree_contains_return(&c.body)
                })
        }
        TreeKind::Try {
            block,
            catches,
            finalizer,
        } => {
            tree_contains_return(block)
                || catches.iter().any(|c| tree_contains_return(&c.body))
                || tree_contains_return(finalizer)
        }
        TreeKind::While { cond, body } | TreeKind::DoWhile { cond, body } => {
            tree_contains_return(cond) || tree_contains_return(body)
        }
        TreeKind::Throw { expr } => tree_contains_return(expr),
        TreeKind::InterpolatedString { args, .. } => args.iter().any(tree_contains_return),
        _ => false,
    }
}

pub(crate) fn tree_has_nlr_to(tree: &Tree, meth: SymbolId) -> bool {
    fn walk(t: &Tree, meth: SymbolId, in_fun: bool) -> bool {
        match &t.kind {
            TreeKind::Return { expr } => {
                (in_fun && (t.sym == meth || t.sym.is_none())) || walk(expr, meth, in_fun)
            }
            TreeKind::Function { vparams, body } => {
                vparams.iter().any(|p| walk(p, meth, in_fun)) || walk(body, meth, true)
            }
            TreeKind::DefDef { vparamss, rhs, .. } => {
                vparamss.iter().flatten().any(|p| walk(p, meth, in_fun)) || walk(rhs, meth, false)
            }
            TreeKind::Select { qual, .. } => walk(qual, meth, in_fun),
            TreeKind::Apply { fun, args } | TreeKind::UnApply { fun, args } => {
                walk(fun, meth, in_fun) || args.iter().any(|a| walk(a, meth, in_fun))
            }
            TreeKind::TypeApply { fun, .. } | TreeKind::Typed { expr: fun, .. } => {
                walk(fun, meth, in_fun)
            }
            TreeKind::Block { stats, expr } => {
                stats.iter().any(|s| walk(s, meth, in_fun)) || walk(expr, meth, in_fun)
            }
            TreeKind::If { cond, thenp, elsep } => {
                walk(cond, meth, in_fun) || walk(thenp, meth, in_fun) || walk(elsep, meth, in_fun)
            }
            TreeKind::Assign { lhs, rhs } => walk(lhs, meth, in_fun) || walk(rhs, meth, in_fun),
            TreeKind::ValDef { rhs, .. } => walk(rhs, meth, in_fun),
            TreeKind::Match { selector, cases } => {
                walk(selector, meth, in_fun)
                    || cases.iter().any(|c| {
                        walk(&c.pat, meth, in_fun)
                            || walk(&c.guard, meth, in_fun)
                            || walk(&c.body, meth, in_fun)
                    })
            }
            TreeKind::Try {
                block,
                catches,
                finalizer,
            } => {
                walk(block, meth, in_fun)
                    || catches.iter().any(|c| walk(&c.body, meth, in_fun))
                    || walk(finalizer, meth, in_fun)
            }
            TreeKind::While { cond, body } | TreeKind::DoWhile { cond, body } => {
                walk(cond, meth, in_fun) || walk(body, meth, in_fun)
            }
            TreeKind::Throw { expr } => walk(expr, meth, in_fun),
            TreeKind::InterpolatedString { args, .. } => args.iter().any(|a| walk(a, meth, in_fun)),
            _ => false,
        }
    }
    walk(tree, meth, false)
}

pub(crate) fn java_deprecated_desc(mods: &scala_rs_parser::Modifiers) -> Option<&'static str> {
    for a in &mods.annotations {
        let p = a.annotation_path();
        if matches!(p.as_str(), "Deprecated" | "java.lang.Deprecated") {
            return Some("Ljava/lang/Deprecated;");
        }
    }
    None
}

pub(crate) fn throw_runtime(asm: &mut Assembler, msg: &str) {
    asm.new_obj("java/lang/RuntimeException");
    asm.dup();
    asm.ldc_string(msg);
    asm.invokespecial(
        "java/lang/RuntimeException",
        "<init>",
        "(Ljava/lang/String;)V",
    );
    asm.athrow();
}

/// A `match` that runs out of cases throws `scala.MatchError` carrying the
/// scrutinee -- what nsc emits, and what `case _: MatchError` catches. A bare
/// `RuntimeException("match error")` was neither. The private runtime emits its
/// own `scala/MatchError` (`runtime.rs`), so both modes throw the same class.
pub(crate) fn throw_match_error(asm: &mut Assembler, ctx: &EmitCtx, sel_ty: &Type, tmp: u16) {
    let sel_ty = sel_ty.widen_constant();
    let sel_sort = jvm_sort(&sel_ty);
    asm.new_obj("scala/MatchError");
    asm.dup();
    load(asm, tmp, sel_sort);
    if sel_sort == JvmSort::Void {
        if ctx.library_abi {
            emit_boxed_unit(asm);
        } else {
            asm.aconst_null();
        }
    } else if is_jvm_primitive(&sel_ty) {
        emit_box(asm, &sel_ty);
    }
    asm.invokespecial("scala/MatchError", "<init>", "(Ljava/lang/Object;)V");
    asm.athrow();
}

pub(crate) fn throw_not_implemented(asm: &mut Assembler) {
    asm.new_obj("scala/NotImplementedError");
    asm.dup();
    asm.invokespecial("scala/NotImplementedError", "<init>", "()V");
    asm.athrow();
}

/// The value a one-armed `if` yields on both paths: `()`, boxed when the
/// recorded type is a reference.
pub(crate) fn push_unit_result(asm: &mut Assembler, ty: &Type) {
    match jvm_sort(ty) {
        JvmSort::Ref => asm.getstatic(BOXED_UNIT, "UNIT", BOXED_UNIT_DESC),
        _ => push_default(asm, ty),
    }
}

pub(crate) fn push_default(asm: &mut Assembler, ty: &Type) {
    match jvm_sort(ty) {
        JvmSort::Void => {}
        JvmSort::Int => asm.iconst(0),
        JvmSort::Long => asm.lconst(0),
        JvmSort::Double => asm.dconst(0.0),
        JvmSort::Float => asm.fconst(0.0),
        JvmSort::Ref => asm.aconst_null(),
    }
}

// ---------------------------------------------------------------------------
// expressions
// ---------------------------------------------------------------------------

pub(crate) fn gen_stat(asm: &mut Assembler, frame: &mut Frame, ctx: &EmitCtx, tree: &Tree) {
    match &tree.kind {
        TreeKind::ValDef { rhs, .. } => {
            let ty = if tree.ty.is_no_type() && !tree.sym.is_none() {
                ctx.st.get(tree.sym).ty.clone()
            } else {
                tree.ty.clone()
            };
            let sort = jvm_sort(&ty);
            // A method-local `lazy val`'s cell: `new scala/runtime/LazyInt()`
            // and nothing else. The initialiser moved into the accessor
            // `lazy_local::lazy_locals` put next to it.
            if !tree.sym.is_none() && ctx.st.local_lazy_cells.contains(&tree.sym) {
                let cn = class_internal(ctx.st, ctx.st.class_sym_of(&ty).unwrap_or(SymbolId::NONE));
                asm.new_obj(&cn);
                asm.dup();
                asm.invokespecial(&cn, "<init>", "()V");
                let slot = frame.alloc(tree.sym, JvmSort::Ref);
                declare_local_ty(asm, ctx.st, slot, &ty);
                store(asm, slot, JvmSort::Ref);
                return;
            }
            if rhs.is_empty() {
                if is_boxed_var(ctx, tree.sym) {
                    push_default(asm, &ty);
                    emit_runtime_ref_create(asm, &ty);
                    let slot = frame.alloc(tree.sym, JvmSort::Ref);
                    store(asm, slot, JvmSort::Ref);
                    return;
                }
                let slot = frame.alloc(tree.sym, sort);
                declare_local_ty(asm, ctx.st, slot, &ty);
                return;
            }
            if sort == JvmSort::Void {
                gen_stat(asm, frame, ctx, rhs);
                frame.alloc(tree.sym, sort);
                return;
            }
            gen_expr(asm, frame, ctx, rhs);
            if is_boxed_var(ctx, tree.sym) {
                emit_runtime_ref_create(asm, &ty);
                let slot = frame.alloc(tree.sym, JvmSort::Ref);
                store(asm, slot, JvmSort::Ref);
                return;
            }
            let slot = frame.alloc(tree.sym, sort);
            // Declared *before* the store: a `var` reassigned in a loop body
            // merges at the loop head, and the merge has to see the declared
            // class on both paths or it degrades to `java/lang/Object`.
            declare_local_ty(asm, ctx.st, slot, &ty);
            store(asm, slot, sort);
        }
        TreeKind::DefDef { .. } | TreeKind::ClassDef { .. } | TreeKind::ModuleDef { .. } => {
            // nested member: not lifted in this pass
        }
        TreeKind::Import { .. } | TreeKind::TypeDef { .. } | TreeKind::Empty => {}
        // nsc's `genLoadIf` with `expectedType = UNIT`: in statement position
        // the value is dropped, so both branches are generated in statement
        // mode. A one-legged `if` is `if (c) t else ()`, whose lub with a
        // value-returning `t` is `Any` (`if (c) { buf += x }`); generating the
        // branches for that type leaves the two paths at different stack
        // heights, because the `else ()` pushes nothing.
        TreeKind::If { cond, thenp, elsep } => {
            gen_if(asm, frame, ctx, cond, thenp, elsep, &Type::Unit);
        }
        // Same story for the other branching forms: two arms whose types only
        // meet at `Any` are not boxed to a common representation when the
        // result is discarded, so each arm has to drop its own value.
        TreeKind::Match { selector, cases } => {
            gen_match(asm, frame, ctx, selector, cases, &Type::Unit);
        }
        // A block's *value* is its last expression, so a discarded block
        // discards that expression -- nsc's `genLoad(block, UNIT)` passes UNIT
        // straight down to it. Falling through to the generic arm below asked
        // `gen_expr` for the block's value and popped it afterwards, which put
        // a branching last expression back in value mode: slick's
        // `QueryInterpolator.appendString`, whose `case '\\' => { pos += 1;
        // if (pos < len) { … match … } }` then generated the inner match for
        // its `Any` lub, and only the arms whose own type was not `Unit` left
        // anything on the stack ("Inconsistent stackmap frames").
        TreeKind::Block { stats, expr } => {
            for s in stats {
                gen_stat(asm, frame, ctx, s);
            }
            gen_stat(asm, frame, ctx, expr);
        }
        TreeKind::Try {
            block,
            catches,
            finalizer,
        } => {
            gen_try(asm, frame, ctx, block, catches, finalizer, &Type::Unit);
        }
        _ => {
            // nsc's `genLoad(tree, UNIT)`: a discarded value is *dropped*, not
            // adapted to the type the typer wrote.
            if let Some(inner) = discarded_unbox(tree) {
                gen_stat(asm, frame, ctx, inner);
                return;
            }
            gen_expr(asm, frame, ctx, tree);
            if is_unit_like(&tree.ty) {
                // A polymorphic method instantiated at `Unit` still *returns* a
                // reference on the JVM (`def id[A](a: A): A` is
                // `(Object)Object`, `PartialFunction[Throwable, Unit].apply`
                // likewise). In value position `unit_leaves_boxed_ref` lets the
                // caller reuse that ref; discarded, it has to go, or a later
                // `goto` merges two different stack heights -- exactly what nsc
                // emits (`invokevirtual id; pop`).
                if unit_stat_leaves_ref(tree, ctx.st)
                    || (ctx.library_abi && unit_call_leaves_ref(tree, ctx.st))
                {
                    asm.pop();
                }
            } else {
                pop_if_value(asm, &tree.ty);
            }
        }
    }
}

/// The operand of a discarded `$unbox`.
///
/// The typer wraps a call whose erased result is `java/lang/Object` in
/// `$unbox` so the primitive the signature promises is on the stack.
/// nsc inserts that adaptation from the *expected* type, and in statement
/// position the expected type is `Unit` — so it emits `invokevirtual put; pop`
/// and never touches the value. Unboxing only to `pop` is not merely wasted
/// work: `java.util.Map[String, Int].put` returns the *previous* value, so
/// `m.put("a", 1)` on a fresh map unboxed `null` and threw
/// `NullPointerException` at the first insert while scalac ran fine. Every
/// erased generic result read for effect has this shape (`map.remove(k)`,
/// `list.set(i, x)`, `buf.remove(0)`, …).
pub(crate) fn discarded_unbox(tree: &Tree) -> Option<&Tree> {
    let TreeKind::Apply { fun, args } = &tree.kind else {
        return None;
    };
    if fun.name() != Some("$unbox") {
        return None;
    }
    args.first()
}

/// nsc: a call whose *declared* return type is `Nothing` can never actually
/// return a value, but the JVM invoke instruction still leaves a real
/// `scala/runtime/Nothing$` (or whatever primitive descriptor) reference on
/// the operand stack -- `Nothing` is `V`-sorted everywhere else in this
/// backend (`jvm_sort`), so nothing downstream expects that phantom slot.
/// Left alone it flows into whatever control-flow join follows -- a `match`
/// arm's `goto`, an `if`'s `goto`, a `try`'s result store, an argument list
/// -- and disagrees with the type the other edges of that join settled on:
/// `VerifyError: Inconsistent stackmap frames`.
///
/// `javap -c` on real scalac output (`scala.sys.error`, `Predef.???`, a
/// user `def die(): Nothing = throw ...`) shows it always follows such a
/// call with `athrow`, in every position -- a `match`/`if` arm, a method
/// body, an argument (`println(sys.error("x"))` never even emits the
/// `invokevirtual println`), an ascription. The one exception is a value
/// already in tail-return position of a method/lambda whose own declared
/// return type is `Nothing`-shaped: there scalac just falls off the end
/// with `areturn`, but appending `athrow` there too is equally valid --
/// `athrow` does not care about the method's declared return descriptor --
/// so one rule covers both.
///
/// `Assembler::athrow` marks the assembler `dead`, and every emitter after
/// that keeps emitting into bytes `drop_dead`/`finish` later truncate, so
/// this composes for free with every caller (statement, argument, arm,
/// tail) without teaching each of them about `Nothing`. Calling it again
/// when the expression already ended in `athrow`/`areturn`/`goto` (an
/// explicit `throw`, `???`, `Breaks.break()`, a `return`) is a harmless
/// no-op for the same reason.
pub(crate) fn gen_expr(asm: &mut Assembler, frame: &mut Frame, ctx: &EmitCtx, tree: &Tree) {
    if crate::gen_tailrec::emit_tail_call(asm, frame, ctx, tree) {
        return;
    }
    gen_expr_inner(asm, frame, ctx, tree);
    // Null$ is an ABI marker, not a JVM subtype of every reference class.
    // Preserve evaluation and checked casts, then expose the JVM's null
    // verification type so a Scala Null can flow to String, arrays or any
    // other reference type. This also checks values read through erased
    // generic results before replacing them with the sole valid Null value.
    if matches!(tree.ty, Type::Null) && asm.top_object().is_some() {
        if asm.top_object() != Some("scala/runtime/Null$") {
            asm.checkcast("scala/runtime/Null$");
        }
        asm.pop();
        asm.aconst_null();
    }
    if matches!(tree.ty, Type::Nothing) {
        // `athrow` only verifies when what is *on the stack* is a `Throwable`.
        // A `Nothing`-typed tree does not always leave one there: a generic
        // method instantiated at `Nothing` (`Nil.flatMap(_ => return false)`),
        // or `Function0.apply` on a lambda that always throws, erases to
        // `Ljava/lang/Object;` (or worse, to `Lscala/collection/immutable/List;`),
        // and the verifier rejected the whole class with `VerifyError: Bad type
        // on operand stack ... is not assignable to 'java/lang/Throwable'`.
        // nsc decides by the *generated* type instead, so it never reaches
        // `athrow` in those positions. The cast costs nothing at run time --
        // the expression cannot complete normally, so it is never executed --
        // and it is a no-op for the common case where the callee's descriptor
        // really is `Lscala/runtime/Nothing$;`.
        let top = asm.top_object().map(str::to_string);
        if let Some(top) = top {
            if top != "scala/runtime/Nothing$" && top != "java/lang/Throwable" {
                asm.checkcast("java/lang/Throwable");
            }
        }
        asm.athrow();
    }
}

pub(crate) fn gen_expr_inner(asm: &mut Assembler, frame: &mut Frame, ctx: &EmitCtx, tree: &Tree) {
    match &tree.kind {
        TreeKind::Empty => {}
        TreeKind::Literal { lit } => gen_literal(asm, lit),
        TreeKind::This { qual } => {
            if let Some(name) = qual {
                load_qualified_this(asm, ctx, name);
            } else if !tree.sym.is_none() && tree.sym != ctx.class_sym {
                // A bare `this` that denotes a class *around* the one being
                // emitted. It gets here from a template's own constructor
                // invocation, which is evaluated outside the template
                // (`new C(this.x) { … }` — the argument belongs to the
                // enclosing expression), so slot 0 holds the still
                // uninitialised new instance and not the `this` that was
                // written.
                load_enclosing_this(asm, ctx, tree.sym);
            } else {
                load_this(asm, ctx);
            }
        }
        TreeKind::Super { .. } => load_this(asm, ctx),
        TreeKind::Ident { .. } => gen_ident(asm, frame, ctx, tree),
        TreeKind::Select { qual, name } => gen_select(asm, frame, ctx, tree, qual, name),
        TreeKind::Apply { fun, args } => gen_apply(asm, frame, ctx, tree, fun, args),
        TreeKind::TypeApply { fun, args } => {
            // `fun` alone (a bare `Select`) still carries the *unsubstituted*
            // type-parameter as its `.ty` (only the outer `TypeApply` node got
            // the concrete type argument substituted in during typechecking),
            // so `asInstanceOf`/`isInstanceOf` must be special-cased here where
            // the resolved type is actually available.
            // `classOf[T]` is a class constant, not a call.
            if !fun.sym.is_none() && matches!(ctx.st.get(fun.sym).intrinsic, Intrinsic::ClassOf) {
                let target = args.first().map(|a| a.ty.clone()).unwrap_or(Type::AnyRef);
                emit_class_constant(asm, ctx, &target);
                return;
            }
            if let TreeKind::Select { qual, .. } = &fun.kind {
                if !fun.sym.is_none() {
                    let ic = ctx.st.get(fun.sym).intrinsic;
                    if matches!(ic, Intrinsic::AsInstanceOf) {
                        gen_expr(asm, frame, ctx, qual);
                        // Both emitters consume a receiver, so a `Unit`
                        // qualifier has to leave its `BoxedUnit` behind:
                        // `().asInstanceOf[Unit]` pops it again, and
                        // `().isInstanceOf[T]` tests it.
                        adapt_unit_qualifier(asm, ctx, qual);
                        // A *primitive* qualifier is not `Object`-compatible,
                        // which is what `emit_as_instance_of` assumes.
                        if emit_prim_qualifier_cast(asm, &qual.ty, &tree.ty) {
                            return;
                        }
                        emit_as_instance_of(asm, ctx, &tree.ty);
                        return;
                    }
                    if matches!(ic, Intrinsic::IsInstanceOf) {
                        gen_expr(asm, frame, ctx, qual);
                        adapt_unit_qualifier(asm, ctx, qual);
                        let target = args.first().map(|a| &a.ty).unwrap_or(&Type::Any);
                        if is_jvm_primitive(&qual.ty) && !is_unit_like(&qual.ty) {
                            emit_box(asm, &qual.ty.widen_constant());
                        }
                        emit_is_instance_of(asm, ctx, target);
                        return;
                    }
                }
            }
            gen_expr(asm, frame, ctx, fun)
        }
        TreeKind::Function { .. } => gen_function(asm, frame, ctx, tree),
        TreeKind::Typed { expr, .. } => {
            gen_expr(asm, frame, ctx, expr);
            // optional checkcast to a class type
            if let Type::Class { sym, .. } = &tree.ty {
                if !is_interface_sym(ctx.st, *sym) {
                    // skip; subclass assignment does not need checkcast
                }
            }
        }
        TreeKind::Block { stats, expr } => {
            for s in stats {
                gen_stat(asm, frame, ctx, s);
            }
            gen_expr(asm, frame, ctx, expr);
        }
        TreeKind::If { cond, thenp, elsep } => {
            gen_if(asm, frame, ctx, cond, thenp, elsep, &tree.ty);
        }
        TreeKind::While { cond, body } => {
            let start = asm.fresh_label();
            let end = asm.fresh_label();
            asm.mark(start);
            gen_expr(asm, frame, ctx, cond);
            asm.ifeq(end);
            gen_stat(asm, frame, ctx, body);
            asm.goto(start);
            asm.mark(end);
        }
        TreeKind::DoWhile { cond, body } => {
            let start = asm.fresh_label();
            asm.mark(start);
            gen_stat(asm, frame, ctx, body);
            gen_expr(asm, frame, ctx, cond);
            asm.ifne(start);
        }
        TreeKind::Assign { lhs, rhs } => gen_assign(asm, frame, ctx, lhs, rhs),
        TreeKind::Match { selector, cases } => {
            gen_match(asm, frame, ctx, selector, cases, &tree.ty);
        }
        TreeKind::New { tpt } => gen_new(asm, frame, ctx, tpt, &[], SymbolId::NONE),
        TreeKind::Return { expr } => {
            if !ctx.method_sym.is_none() && (tree.sym == ctx.method_sym || tree.sym.is_none()) {
                if !expr.is_empty() {
                    gen_expr(asm, frame, ctx, expr);
                    if is_unit_like(&ctx.ret_ty) {
                        pop_if_value(asm, &expr.ty);
                    }
                }
                match frame.finally_exits.last().copied() {
                    // Inside a `try ... finally`: park the value and let the
                    // finalizers run before the method actually returns.
                    Some(exit) => {
                        if !is_unit_like(&ctx.ret_ty) {
                            let sort = jvm_sort(&ctx.ret_ty);
                            let slot = frame.return_slot(sort);
                            store(asm, slot, sort);
                        }
                        asm.goto(exit);
                    }
                    None => emit_return(asm, &ctx.ret_ty),
                }
                // Dead code after `return` still needs a dummy for the method
                // epilogue (and for StackMapTable at the next instruction).
                push_default(asm, &ctx.ret_ty);
            } else {
                emit_nonlocal_return(asm, frame, ctx, expr);
                push_default(asm, &ctx.ret_ty);
            }
        }
        TreeKind::Throw { expr } => {
            gen_expr(asm, frame, ctx, expr);
            asm.athrow();
            push_default(asm, &tree.ty);
        }
        TreeKind::Try {
            block,
            catches,
            finalizer,
        } => {
            gen_try(asm, frame, ctx, block, catches, finalizer, &tree.ty);
        }
        TreeKind::InterpolatedString {
            prefix,
            parts,
            args,
        } => {
            if prefix == "f" {
                gen_f_interpolated(asm, frame, ctx, parts, args);
            } else {
                gen_interpolated(asm, frame, ctx, parts, args);
            }
        }
        TreeKind::ValDef { .. } => {
            gen_stat(asm, frame, ctx, tree);
        }
        TreeKind::Import { .. } | TreeKind::TypeDef { .. } => {}
        _ => {
            throw_runtime(
                asm,
                &format!(
                    "unimplemented expression: {}",
                    tree.name().unwrap_or("<tree>")
                ),
            );
            push_default(asm, &tree.ty);
        }
    }
}

pub(crate) fn gen_literal(asm: &mut Assembler, lit: &Lit) {
    match lit {
        Lit::Unit => {}
        Lit::Boolean(b) => asm.iconst(if *b { 1 } else { 0 }),
        Lit::Int(n) => asm.iconst(*n),
        Lit::Long(n) => asm.lconst(*n),
        Lit::Float(n) => asm.fconst(*n),
        Lit::Double(n) => asm.dconst(*n),
        Lit::Char(c) => asm.iconst(*c as i32),
        Lit::String(s) => asm.ldc_string(s),
        Lit::Null => asm.aconst_null(),
        // `'foo` is a `scala.Symbol`, not its name. Pushing the bare string
        // type-checked (the literal's type is `Symbol`) and then printed
        // `foo` where scalac prints `Symbol(foo)`; any actual `Symbol` member
        // on it would have been a `NoSuchMethodError` at run time.
        // `Symbol.apply` interns, so `'foo eq 'foo` still holds.
        Lit::Symbol(s) => {
            asm.getstatic("scala/Symbol$", "MODULE$", "Lscala/Symbol$;");
            asm.ldc_string(s);
            asm.invokevirtual(
                "scala/Symbol$",
                "apply",
                "(Ljava/lang/String;)Lscala/Symbol;",
            );
        }
    }
}

pub(crate) fn load_this(asm: &mut Assembler, ctx: &EmitCtx) {
    if let Some(slot) = ctx.outer_slot {
        asm.aload(slot);
        return;
    }
    if let Some((lclass, field, desc)) = ctx.outer {
        asm.aload(0);
        asm.getfield(lclass, field, desc);
        return;
    }
    // Inside a value class's `$extension` static, slot 0 holds the *underlying*
    // value. Anything that wants the instance itself -- a lambda's `$outer`
    // above all -- gets a fresh box, which is what nsc's own
    // `new C(u)`-on-demand amounts to. Without it slick's
    // `ConfigExtensionMethods.toProperties` handed a `Config` where its
    // lambda's constructor wanted a `ConfigExtensionMethods`.
    if let Some((cls, ctor, sort)) = &ctx.value_ext {
        asm.new_obj(cls);
        asm.dup();
        load(asm, 0, *sort);
        asm.invokespecial(cls, "<init>", ctor);
        return;
    }
    asm.aload(0);
}

/// A bare `this` that denotes `target`, a class the one being emitted is
/// lexically inside.
///
/// An `object`'s single instance is reachable without one; otherwise walk the
/// `$outer` chain, which in the pre-super part of an `<init>` starts from the
/// constructor's own `$outer` argument rather than from a `getfield` the
/// verifier would reject. Outside that region `load_this` is already right —
/// `ctx.outer` / `ctx.outer_slot` say how a lambda or a trait's static gets
/// at its instance — so leave those alone.
pub(crate) fn load_enclosing_this(asm: &mut Assembler, ctx: &EmitCtx, target: SymbolId) {
    if is_module_class(ctx.st, target) {
        load_module_instance(asm, ctx, target);
        return;
    }
    if ctx.presuper_outer.is_none() {
        load_this(asm, ctx);
        return;
    }
    let (mut cur, _) = start_outer_walk(asm, ctx, true);
    while !cur.is_none() && cur != target {
        let Some(outer) = enclosing_instance(ctx.st, cur) else {
            break;
        };
        let f = outer_field_class(ctx.st, cur).unwrap_or(outer);
        load_outer_of(asm, ctx.st, cur, f);
        cur = outer;
    }
}

pub(crate) fn load_qualified_this(asm: &mut Assembler, ctx: &EmitCtx, name: &str) {
    let target = ctx
        .st
        .enclosing_class_named(ctx.class_sym, name)
        .unwrap_or(ctx.class_sym);
    let (mut cur, _) = start_outer_walk(asm, ctx, ctx.class_sym != target);
    while !cur.is_none() && cur != target {
        let Some(outer) = enclosing_instance(ctx.st, cur) else {
            break;
        };
        let f = outer_field_class(ctx.st, cur).unwrap_or(outer);
        load_outer_of(asm, ctx.st, cur, f);
        cur = outer;
    }
}

pub(crate) fn gen_ident(asm: &mut Assembler, frame: &mut Frame, ctx: &EmitCtx, tree: &Tree) {
    if matches!(&tree.kind, TreeKind::Ident { name } if name == "$classOf") {
        gen_java_class_of(asm, ctx, &tree.ty);
        return;
    }
    let id = tree.sym;
    if id.is_none() {
        throw_runtime(
            asm,
            &format!("unresolved ident {}", tree.name().unwrap_or("?")),
        );
        push_default(asm, &tree.ty);
        return;
    }
    let ic = ctx.st.get(id).intrinsic;
    if matches!(ic, Intrinsic::NotImplemented) {
        if ctx.library_abi {
            emit_predef_nyi(asm);
            // `Nothing` is handled generically by `gen_expr`'s `athrow`-append.
            if is_unit_like(&tree.ty) {
                asm.pop();
            }
        } else {
            throw_not_implemented(asm);
            push_default(asm, &tree.ty);
        }
        return;
    }
    if let Some((slot, sort)) = frame.get(id) {
        if is_boxed_var(ctx, id) {
            load(asm, slot, JvmSort::Ref);
            load_runtime_ref_elem(asm, ctx, &ctx.st.get(id).ty);
            return;
        }
        load(asm, slot, sort);
        return;
    }
    let sym = ctx.st.get(id);
    if sym.flags.contains(Flags::LAZY) && sym.kind == SymKind::Term {
        let owner_sym = sym.owner;
        if is_module_class(ctx.st, owner_sym)
            && module_class_id(ctx.st, owner_sym) != module_class_id(ctx.st, ctx.class_sym)
        {
            load_module_instance(asm, ctx, module_class_id(ctx.st, owner_sym));
        } else {
            load_owner_instance(asm, ctx, owner_sym);
        }
        let owner = class_internal(ctx.st, owner_sym);
        let desc = format!("(){}", jvm_desc(ctx.st, &sym.ty));
        if is_trait_owned_term(ctx.st, id) {
            asm.invokeinterface(&owner, &sym.name, &desc);
        } else {
            asm.invokevirtual(&owner, &sym.name, &desc);
        }
        return;
    }
    match sym.kind {
        SymKind::Term => {
            let owner = sym.owner;
            // A template's self alias (`trait T { self: P => … }`) denotes the
            // template's own `this`. Read from a class nested inside `T` that
            // is the *outer* instance, not this one, so it has to be reached
            // through `$outer` like any other enclosing-instance reference.
            if ctx.st.get(owner).self_alias == Some(id) {
                load_self_alias_instance(asm, ctx, owner);
                if let Some(cls) = ctx.st.class_sym_of(&sym.ty) {
                    if !is_owner_compatible(ctx.st, owner, cls)
                        && (matches!(ctx.st.get(cls).kind, SymKind::Class | SymKind::ModuleClass)
                            || is_interface_sym(ctx.st, cls))
                    {
                        asm.checkcast(&class_internal(ctx.st, cls));
                    }
                }
                return;
            }
            if sym.flags.contains(Flags::SYNTHETIC) && !sym.flags.contains(Flags::PARAM) {
                load_this(asm, ctx);
                if let Some(cls) = ctx.st.class_sym_of(&sym.ty) {
                    maybe_checkcast_owner(asm, ctx, cls);
                }
                return;
            }
            if is_module_class(ctx.st, owner)
                && module_class_id(ctx.st, owner) != module_class_id(ctx.st, ctx.class_sym)
            {
                load_module_instance(asm, ctx, module_class_id(ctx.st, owner));
            } else if is_private_this(ctx.st, id) {
                load_self_alias_instance(asm, ctx, owner);
            } else {
                load_owner_instance(asm, ctx, owner);
            }
            if is_trait_owned_term(ctx.st, id) {
                let owner = class_internal(ctx.st, owner);
                let desc = format!("(){}", jvm_desc(ctx.st, &sym.ty));
                asm.invokeinterface(&owner, &sym.name, &desc);
            } else {
                let owner = class_internal(ctx.st, owner);
                let desc = jvm_desc_val(ctx.st, &sym.ty);
                // A library constructor field is private; `jvm_name` holds the
                // accessor to call instead (`StringContext.parts`). Its
                // *result* is a method result, so `Unit` is `V` there even
                // though the field itself is a `BoxedUnit`.
                if reads_via_accessor(ctx.st, id) {
                    asm.invokevirtual(
                        &owner,
                        &sym.name,
                        &format!("(){}", jvm_desc(ctx.st, &sym.ty)),
                    );
                } else if sym.jvm_name.is_empty() {
                    emit_getfield(asm, &owner, &sym.name, &desc);
                } else {
                    let acc = sym.jvm_name.clone();
                    asm.invokevirtual(&owner, &acc, &format!("(){}", jvm_desc(ctx.st, &sym.ty)));
                }
            }
        }
        SymKind::Module | SymKind::ModuleClass => {
            load_module_instance(asm, ctx, module_class_id(ctx.st, id));
        }
        SymKind::Class => {
            // Java classes have no companion MODULE$. Scala `Foo.bar` still
            // loads `Foo$` when a companion exists.
            //
            // The `JAVA` flag alone does not settle it: a class stubbed by
            // `find_or_stub_java_class` before its class file was read keeps
            // the flag for the rest of the run even when the class file says
            // Scala. `cats.effect.IO` is one, and `IO.blocking(…)` came out
            // with no receiver at all — `Operand stack underflow` in slick's
            // `slick.cats.Database$`. What does settle it is the companion's
            // own JVM name: only a Scala companion is this class's `Foo$`.
            let want = format!("{}$", class_internal(ctx.st, id));
            let Some(comp) = ctx
                .st
                .companion_module(id)
                .map(|m| module_class_id(ctx.st, m))
                .filter(|c| class_internal(ctx.st, *c) == want)
            else {
                return;
            };
            // A companion of a class nested in a class is itself an inner
            // `object`, reached through the enclosing instance's accessor.
            if member_module_outer(ctx.st, comp).is_some() {
                load_module_instance(asm, ctx, comp);
                return;
            }
            asm.getstatic(&want, "MODULE$", &format!("L{want};"));
        }
        SymKind::Method => {
            let owner = ctx.st.get(id).owner;
            if is_module_class(ctx.st, owner) {
                load_module_instance(asm, ctx, module_class_id(ctx.st, owner));
            } else if ctx.st.get(owner).kind == SymKind::Package {
                // A package-object member (`scala.math.Pi`,
                // `scala.reflect.runtime.universe`) is reached through the
                // static forwarder on `<pkg>/package`, which takes no
                // receiver. Falling through to `load_owner_instance` pushed
                // `this` and produced a `VerifyError` at run time.
            } else if is_private_this(ctx.st, id) {
                load_self_alias_instance(asm, ctx, owner);
            } else {
                load_owner_instance(asm, ctx, owner);
            }
            invoke_method(asm, ctx, id, Some(&tree.ty));
        }
        SymKind::Package => {
            // `math.Pi` reads the package object; the package itself has no
            // runtime representation.
            match package_object_module(ctx.st, id) {
                Some(mcls) => {
                    let jvm = class_internal(ctx.st, mcls);
                    asm.getstatic(&jvm, "MODULE$", &format!("L{jvm};"));
                }
                None => {
                    throw_runtime(asm, &format!("cannot load {}", sym.name));
                    push_default(asm, &tree.ty);
                }
            }
        }
        _ => {
            throw_runtime(asm, &format!("cannot load {}", sym.name));
            push_default(asm, &tree.ty);
        }
    }
}

pub(crate) fn gen_structural_call(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    recv: &Tree,
    name: &str,
    args: &[Tree],
    result: &Type,
) {
    // Scala 2.13 reflective call: getClass.getMethod(name, Class[]).invoke(recv, Object[])
    gen_expr(asm, frame, ctx, recv);
    asm.dup();
    asm.invokevirtual("java/lang/Object", "getClass", "()Ljava/lang/Class;");
    asm.ldc_string(&encode_method_name(name));
    asm.iconst(args.len() as i32);
    asm.anewarray("java/lang/Class");
    for (i, a) in args.iter().enumerate() {
        asm.dup();
        asm.iconst(i as i32);
        gen_java_class_of(asm, ctx, &a.ty);
        asm.aastore();
    }
    asm.invokevirtual(
        "java/lang/Class",
        "getMethod",
        "(Ljava/lang/String;[Ljava/lang/Class;)Ljava/lang/reflect/Method;",
    );
    asm.swap();
    asm.iconst(args.len() as i32);
    asm.anewarray("java/lang/Object");
    for (i, a) in args.iter().enumerate() {
        asm.dup();
        asm.iconst(i as i32);
        gen_expr(asm, frame, ctx, a);
        emit_box(asm, &a.ty);
        asm.aastore();
    }
    asm.invokevirtual(
        "java/lang/reflect/Method",
        "invoke",
        "(Ljava/lang/Object;[Ljava/lang/Object;)Ljava/lang/Object;",
    );
    match result {
        Type::Int
        | Type::Boolean
        | Type::Long
        | Type::Double
        | Type::Char
        | Type::Float
        | Type::Byte
        | Type::Short => {
            emit_unbox(asm, result);
        }
        Type::Unit | Type::NoType => {
            asm.pop();
        }
        Type::String => {
            asm.checkcast("java/lang/String");
        }
        _ => {
            if let Some(cn) = checkcast_internal(ctx.st, result) {
                asm.checkcast(&cn);
            }
        }
    }
}

pub(crate) fn gen_java_class_of(asm: &mut Assembler, ctx: &EmitCtx, ty: &Type) {
    match ty {
        Type::Int => asm.getstatic("java/lang/Integer", "TYPE", "Ljava/lang/Class;"),
        Type::Boolean => asm.getstatic("java/lang/Boolean", "TYPE", "Ljava/lang/Class;"),
        Type::Long => asm.getstatic("java/lang/Long", "TYPE", "Ljava/lang/Class;"),
        Type::Double => asm.getstatic("java/lang/Double", "TYPE", "Ljava/lang/Class;"),
        Type::Float => asm.getstatic("java/lang/Float", "TYPE", "Ljava/lang/Class;"),
        Type::Char => asm.getstatic("java/lang/Character", "TYPE", "Ljava/lang/Class;"),
        Type::Byte => asm.getstatic("java/lang/Byte", "TYPE", "Ljava/lang/Class;"),
        Type::Short => asm.getstatic("java/lang/Short", "TYPE", "Ljava/lang/Class;"),
        Type::Unit | Type::NoType => asm.getstatic("java/lang/Void", "TYPE", "Ljava/lang/Class;"),
        Type::String => asm.ldc_class("java/lang/String"),
        Type::Class { sym, .. } | Type::ModuleRef(sym) => {
            asm.ldc_class(&class_internal(ctx.st, *sym));
        }
        Type::Named { name, .. } => asm.ldc_class(&name.replace('.', "/")),
        // An array's class-literal constant is spelled with its *descriptor*
        // (`[I`, `[[I`, `[Ljava/lang/String;`), not an internal name. Falling
        // through to `java/lang/Object` gave `Array(Array(1, 2), Array(3, 4))`
        // a `ClassTag[Object]`, so `Array.apply` built an `Object[]` and the
        // `checkcast [[I` on the result threw `ClassCastException`.
        Type::Array(_) => asm.ldc_class(&jvm_desc(ctx.st, ty)),
        // A tuple and a function are classes like any other, and the class
        // literal a `ClassTag` is built from decides what `Array.apply`
        // allocates. Falling through to `java/lang/Object` made
        // `Array[(Int, String)](1 -> "one")` an `Object[]`, and the
        // `checkcast [Lscala/Tuple2;` the caller emits on the result threw
        // `ClassCastException` -- with nothing wrong at the type level.
        Type::Tuple(ts) => asm.ldc_class(&format!("scala/Tuple{}", ts.len())),
        Type::Function { params, .. } => asm.ldc_class(&format!("scala/Function{}", params.len())),
        // A type annotation says nothing about the class underneath
        // (`Array[T @uncheckedVariance]`), and a literal type's class is its
        // underlying type's.
        Type::Annotated { tpe, .. } => gen_java_class_of(asm, ctx, tpe),
        Type::Constant(lit) => gen_java_class_of(asm, ctx, &Type::lit_underlying(lit)),
        _ => asm.ldc_class("java/lang/Object"),
    }
}

/// Push `<pkg>/package$.MODULE$` for `scala.math.Pi` and friends.
///
/// The typer folds a package object's members into the package symbol, so
/// `scala.math.Pi` is a `Select` whose qualifier is the *package* `scala.math`
/// -- which has no runtime value and emits nothing -- while the member itself
/// is owned by the package object's module class. Emitting the qualifier then
/// left the stack empty under an `invokevirtual`, a `VerifyError` at run time.
///
/// Returns `false` when this is not that shape, and the caller emits the
/// qualifier as usual.
pub(crate) fn load_package_object_receiver(
    asm: &mut Assembler,
    ctx: &EmitCtx,
    qual: &Tree,
    owner: SymbolId,
) -> bool {
    if qual.sym.is_none() || ctx.st.get(qual.sym).kind != SymKind::Package {
        return false;
    }
    if !is_module_class(ctx.st, owner) {
        return false;
    }
    let jvm = class_internal(ctx.st, module_class_id(ctx.st, owner));
    asm.getstatic(&jvm, "MODULE$", &format!("L{jvm};"));
    true
}

pub(crate) fn gen_select(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    tree: &Tree,
    qual: &Tree,
    name: &str,
) {
    if name == "length" && matches!(qual.ty, Type::Array(_)) {
        gen_expr(asm, frame, ctx, qual);
        asm.arraylength();
        return;
    }
    if name == "length" && !tree.sym.is_none() && ctx.st.get(tree.sym).owner == ctx.st.array_sym {
        // Generic `Array[T]` erases to Object; nsc uses ScalaRunTime.array_length.
        if ctx.library_abi {
            asm.getstatic(
                "scala/runtime/ScalaRunTime$",
                "MODULE$",
                "Lscala/runtime/ScalaRunTime$;",
            );
            gen_expr(asm, frame, ctx, qual);
            asm.invokevirtual(
                "scala/runtime/ScalaRunTime$",
                "array_length",
                "(Ljava/lang/Object;)I",
            );
        } else {
            gen_expr(asm, frame, ctx, qual);
            asm.arraylength();
        }
        return;
    }
    if matches!(qual.ty, Type::Refined { .. }) && tree.sym.is_none() {
        gen_structural_call(asm, frame, ctx, qual, name, &[], &tree.ty);
        return;
    }
    if !tree.sym.is_none() {
        let s = ctx.st.get(tree.sym);
        if s.flags.contains(Flags::LAZY) && s.kind == SymKind::Term {
            gen_select_receiver(asm, frame, ctx, qual, s.owner);
            let owner = class_internal(ctx.st, s.owner);
            let desc = format!("(){}", jvm_desc(ctx.st, &s.ty));
            // A trait's `lazy val` is an interface member; every implementing
            // class carries its own accessor (nsc's mixin phase).
            if is_trait_owned_term(ctx.st, tree.sym) {
                asm.invokeinterface(&owner, &s.name, &desc);
            } else {
                asm.invokevirtual(&owner, &s.name, &desc);
            }
            return;
        }
        match s.kind {
            SymKind::Term => {
                if s.flags.contains(Flags::STATIC) {
                    let owner = class_internal(ctx.st, s.owner);
                    let desc = if !s.jvm_name.is_empty() && !s.jvm_name.starts_with('(') {
                        s.jvm_name.clone()
                    } else {
                        jvm_desc_val(ctx.st, &s.ty)
                    };
                    asm.getstatic(&owner, &s.name, &desc);
                    if desc == BOXED_UNIT_DESC {
                        asm.pop();
                    }
                    maybe_cast_erased_load(asm, ctx, &s.ty, &tree.ty);
                    return;
                }
                if !load_package_object_receiver(asm, ctx, qual, s.owner) {
                    gen_select_receiver(asm, frame, ctx, qual, s.owner);
                    checkcast_refined_receiver(asm, ctx, &qual.ty, tree.sym);
                }
                if is_trait_owned_term(ctx.st, tree.sym) {
                    let owner = class_internal(ctx.st, s.owner);
                    let desc = format!("(){}", jvm_desc(ctx.st, &s.ty));
                    asm.invokeinterface(&owner, &s.name, &desc);
                } else {
                    let owner = class_internal(ctx.st, s.owner);
                    let desc = jvm_desc_val(ctx.st, &s.ty);
                    if reads_via_accessor(ctx.st, tree.sym) {
                        asm.invokevirtual(
                            &owner,
                            &s.name,
                            &format!("(){}", jvm_desc(ctx.st, &s.ty)),
                        );
                    } else if s.jvm_name.is_empty() {
                        emit_getfield(asm, &owner, &s.name, &desc);
                    } else {
                        let acc = s.jvm_name.clone();
                        asm.invokevirtual(&owner, &acc, &format!("(){}", jvm_desc(ctx.st, &s.ty)));
                    }
                    maybe_cast_erased_load(asm, ctx, &s.ty, &tree.ty);
                }
                return;
            }
            SymKind::Method => {
                let ic = s.intrinsic;
                if !s.flags.contains(Flags::STATIC) {
                    if !load_package_object_receiver(asm, ctx, qual, s.owner) {
                        gen_select_receiver(asm, frame, ctx, qual, s.owner);
                        checkcast_refined_receiver(asm, ctx, &qual.ty, tree.sym);
                        // The call names `declaring_class` when the owner's
                        // own class file does not reach the method
                        // (`Symbol::declaring_class`), so the receiver has to
                        // be cast to that class first -- the same step the
                        // `Apply` path takes in
                        // `checkcast_erased_method_receiver`. Paren-less, it
                        // was missing: `u.Expr` left a `JavaUniverse` on the
                        // stack under `invokevirtual
                        // scala/reflect/api/Universe.Expr()` and the verifier
                        // threw the whole method out.
                        let dc = s.declaring_class.clone();
                        if !matches!(qual.kind, TreeKind::Super { .. }) {
                            if dc.is_empty() {
                                // The receiver's *own* erased class may not
                                // reach the owner either, with no
                                // `declaring_class` to say so: `type TypeName
                                // >: Null <: TypeNameApi with Name` erases to
                                // the first parent, and `toTermName` is
                                // `NameApi`'s. Stack-aware, so it costs three
                                // bytes only where the verifier needs them.
                                checkcast_method_receiver_sym(asm, ctx, tree.sym, true);
                            } else {
                                asm.checkcast(&dc);
                            }
                        }
                    }
                    // Paren-less call to an `ArrayOps` member with no
                    // `$extension` in nsc 2.13.16 (`toList`, `sum`, …); see
                    // the matching Apply-path insertion above `invoke_value_extension`.
                    if is_array_ops_wrap_method(ctx.st, tree.sym) {
                        emit_array_wrap_to_iterable_ops(asm, ctx);
                    }
                }
                if matches!(qual.kind, TreeKind::Super { .. }) {
                    invoke_super(asm, ctx, tree.sym);
                } else if matches!(ic, Intrinsic::AnyHash) {
                    emit_any_hash(asm, &qual.ty);
                } else if matches!(ic, Intrinsic::GetClass) {
                    // Same as the `Apply` path: `1.getClass` is `Integer.TYPE`
                    // and `().getClass` is `Void.TYPE`, not the box's class.
                    // Paren-less, this fell through to a plain
                    // `Object.getClass` and printed `class java.lang.Integer`.
                    emit_get_class(asm, ctx, &qual.ty);
                } else if matches!(ic, Intrinsic::StringToInt) {
                    asm.invokestatic("java/lang/Integer", "parseInt", "(Ljava/lang/String;)I");
                } else if matches!(ic, Intrinsic::StringToLong) {
                    asm.invokestatic("java/lang/Long", "parseLong", "(Ljava/lang/String;)J");
                } else if matches!(ic, Intrinsic::StringToDouble) {
                    asm.invokestatic("java/lang/Double", "parseDouble", "(Ljava/lang/String;)D");
                } else if matches!(ic, Intrinsic::IntToLong) {
                    asm.i2l();
                } else if matches!(ic, Intrinsic::IntToDouble) {
                    asm.i2d();
                } else if matches!(ic, Intrinsic::LongToDouble) {
                    asm.l2d();
                } else if matches!(ic, Intrinsic::IntToFloat) {
                    asm.i2f();
                } else if matches!(ic, Intrinsic::LongToFloat) {
                    asm.l2f();
                } else if matches!(ic, Intrinsic::FloatToDouble) {
                    asm.f2d();
                } else if matches!(ic, Intrinsic::IntToByte) {
                    asm.i2b();
                } else if matches!(ic, Intrinsic::IntToShort) {
                    asm.i2s();
                } else if let Intrinsic::NumConv(code) = ic {
                    emit_num_conv(asm, code);
                } else if matches!(ic, Intrinsic::AsInstanceOf) {
                    // Reached without going through the `TypeApply` special case
                    // in `gen_expr` (e.g. a bare `.asInstanceOf` reference); the
                    // best available type here is `tree.ty`, which may still be
                    // the unsubstituted type parameter for a degenerate caller.
                    emit_as_instance_of(asm, ctx, &tree.ty);
                } else if matches!(ic, Intrinsic::IsInstanceOf) {
                    emit_is_instance_of(asm, ctx, &tree.ty);
                } else if matches!(ic, Intrinsic::NotImplemented) {
                    if ctx.library_abi {
                        // Receiver was already pushed; Predef.??? is MODULE$.???().
                        asm.pop();
                        emit_predef_nyi(asm);
                        // `Nothing` is handled generically by `gen_expr`'s
                        // `athrow`-append.
                        if is_unit_like(&tree.ty) {
                            asm.pop();
                        }
                    } else {
                        throw_not_implemented(asm);
                        push_default(asm, &tree.ty);
                    }
                } else if ctx.st.is_value_class(ctx.st.get(tree.sym).owner) {
                    invoke_value_extension(asm, ctx, tree.sym, Some(&tree.ty), false);
                } else {
                    // `x.toString` on an `Int` dispatches on
                    // `java/lang/Integer` (or `java/lang/Object` for the
                    // inherited one), so box the primitive receiver.
                    let owner_jvm = class_internal(ctx.st, s.owner);
                    if !s.flags.contains(Flags::STATIC)
                        && is_jvm_primitive(&qual.ty)
                        && !is_unit_like(&qual.ty)
                        && (is_boxed_primitive(&owner_jvm) || owner_jvm == "java/lang/Object")
                    {
                        emit_box(asm, &qual.ty);
                    }
                    invoke_method(asm, ctx, tree.sym, Some(&tree.ty));
                }
                return;
            }
            SymKind::Module | SymKind::ModuleClass => {
                let mcls = module_class_id(ctx.st, tree.sym);
                // `o.P` on a member `object`: the qualifier *is* the enclosing
                // instance, and the instance comes from its `P()` accessor.
                if let Some(outer) = member_module_outer(ctx.st, mcls) {
                    gen_expr(asm, frame, ctx, qual);
                    let known = ctx
                        .st
                        .class_sym_of(&qual.ty)
                        .is_some_and(|q| is_owner_compatible(ctx.st, q, outer));
                    if !known {
                        asm.checkcast(&class_internal(ctx.st, outer));
                    }
                    invoke_module_accessor(asm, ctx.st, outer, mcls);
                    return;
                }
                let jvm = class_internal(ctx.st, mcls);
                asm.getstatic(&jvm, "MODULE$", &format!("L{jvm};"));
                return;
            }
            SymKind::Package => {
                // Package prefixes of `scala.collection.immutable.Queue` have
                // no runtime value; the module Select loads MODULE$.
                return;
            }
            _ => {}
        }
    }
    if let Some(cid) = ctx.st.class_sym_of(&qual.ty) {
        gen_expr(asm, frame, ctx, qual);
        let owner = class_internal(ctx.st, cid);
        let desc = jvm_desc_val(ctx.st, &tree.ty);
        emit_getfield(asm, &owner, name, &desc);
        return;
    }
    throw_runtime(asm, &format!("select {name}"));
    push_default(asm, &tree.ty);
}

pub(crate) fn gen_assign(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    lhs: &Tree,
    rhs: &Tree,
) {
    match &lhs.kind {
        TreeKind::Ident { .. } => {
            let id = lhs.sym;
            if is_boxed_var(ctx, id) {
                if let Some((slot, _)) = frame.get(id) {
                    load(asm, slot, JvmSort::Ref);
                    gen_expr(asm, frame, ctx, rhs);
                    store_runtime_ref_elem(asm, &ctx.st.get(id).ty);
                    return;
                }
            }
            if let Some((slot, sort)) = frame.get(id) {
                gen_expr(asm, frame, ctx, rhs);
                store(asm, slot, sort);
                return;
            }
            if !id.is_none() {
                let s = ctx.st.get(id);
                // A trait's `var` lives on the implementing class, not on the
                // interface: assign through nsc's `v_$eq` accessor. Reaching
                // for the field would be a `NoSuchFieldError` on the trait.
                if is_trait_owned_term(ctx.st, id) {
                    load_owner_instance(asm, ctx, s.owner);
                    gen_expr(asm, frame, ctx, rhs);
                    let owner = class_internal(ctx.st, s.owner);
                    let vd = jvm_desc_val(ctx.st, &s.ty);
                    fill_boxed_unit_slot(asm, &vd);
                    asm.invokeinterface(&owner, &var_setter_name(&s.name), &format!("({vd})V"));
                    return;
                }
                // Inherited `var` of a separately compiled superclass: its
                // field is private there, so go through the setter.
                if s.via_accessor {
                    load_owner_instance(asm, ctx, s.owner);
                    gen_expr(asm, frame, ctx, rhs);
                    let owner = class_internal(ctx.st, s.owner);
                    let vd = jvm_desc_val(ctx.st, &s.ty);
                    fill_boxed_unit_slot(asm, &vd);
                    asm.invokevirtual(&owner, &var_setter_name(&s.name), &format!("({vd})V"));
                    return;
                }
                // `var` of an *enclosing* class assigned from an anonymous or
                // local class: the receiver is that instance, reached along
                // `$outer`, not this one. Pushing `this` put the wrong object
                // under the `putfield` and the verifier rejected the method.
                // The read path (`gen_ident`) already walks out this way.
                if is_private_this(ctx.st, id) {
                    load_self_alias_instance(asm, ctx, s.owner);
                } else {
                    load_owner_instance(asm, ctx, s.owner);
                }
                gen_expr(asm, frame, ctx, rhs);
                emit_putfield_from_expr(
                    asm,
                    &class_internal(ctx.st, s.owner),
                    &s.name,
                    &jvm_desc_val(ctx.st, &s.ty),
                );
                return;
            }
            gen_expr(asm, frame, ctx, rhs);
            pop_if_value(asm, &rhs.ty);
        }
        TreeKind::Select { qual, name } => {
            gen_expr(asm, frame, ctx, qual);
            gen_expr(asm, frame, ctx, rhs);
            if !lhs.sym.is_none() && is_trait_owned_term(ctx.st, lhs.sym) {
                let s = ctx.st.get(lhs.sym);
                let owner = class_internal(ctx.st, s.owner);
                let vd = jvm_desc_val(ctx.st, &s.ty);
                fill_boxed_unit_slot(asm, &vd);
                asm.invokeinterface(&owner, &var_setter_name(&s.name), &format!("({vd})V"));
                return;
            }
            // A `var` of a separately compiled class: scalac made the field
            // private, so the write is `v_$eq(x)` exactly as the read is
            // `v()`. See `Symbol::via_accessor`.
            if !lhs.sym.is_none() && ctx.st.get(lhs.sym).via_accessor {
                let s = ctx.st.get(lhs.sym);
                let owner = class_internal(ctx.st, s.owner);
                let vd = jvm_desc_val(ctx.st, &s.ty);
                fill_boxed_unit_slot(asm, &vd);
                asm.invokevirtual(&owner, &var_setter_name(&s.name), &format!("({vd})V"));
                return;
            }
            let owner = if !lhs.sym.is_none() {
                class_internal(ctx.st, ctx.st.get(lhs.sym).owner)
            } else if let Some(cid) = ctx.st.class_sym_of(&qual.ty) {
                class_internal(ctx.st, cid)
            } else {
                ctx.class_name.to_string()
            };
            let desc = if !lhs.ty.is_no_type() {
                jvm_desc_val(ctx.st, &lhs.ty)
            } else {
                jvm_desc_val(ctx.st, &rhs.ty)
            };
            emit_putfield_from_expr(asm, &owner, name, &desc);
        }
        _ => {
            gen_expr(asm, frame, ctx, rhs);
            pop_if_value(asm, &rhs.ty);
        }
    }
}

pub(crate) fn gen_if(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    cond: &Tree,
    thenp: &Tree,
    elsep: &Tree,
    result_ty: &Type,
) {
    gen_expr(asm, frame, ctx, cond);
    let else_l = asm.fresh_label();
    let end_l = asm.fresh_label();
    if let Some(n) = join_class_of(ctx.st, result_ty) {
        asm.set_join_class(end_l, &n);
    }
    // `if (c) e` with no `else` and a non-`Unit` recorded type. nsc gives such
    // an expression the type `Unit`, so the branch's value is dropped and `()`
    // is what it yields. Emitting the then-branch's value while the (empty)
    // else path pushed nothing left the two paths at different stack heights
    // and the JVM rejected the method ("Inconsistent stackmap frames"):
    // slick's `runPhase` ends in `if (GlobalConfig.verifyTypes && …) (new
    // VerifyTypes(…)).apply(s2)`, whose branch value is a `CompilerState`.
    asm.ifeq(else_l);
    if is_unit_like(result_ty) {
        gen_stat(asm, frame, ctx, thenp);
    } else {
        gen_expr(asm, frame, ctx, thenp);
        pad_unit_branch(asm, thenp, result_ty);
    }
    asm.goto(end_l);
    asm.mark(else_l);
    if is_unit_like(result_ty) {
        gen_stat(asm, frame, ctx, elsep);
    } else {
        gen_expr(asm, frame, ctx, elsep);
        pad_unit_branch(asm, elsep, result_ty);
    }
    asm.mark(end_l);
}

/// A branch of a non-`Unit` `if` whose own value is `()` leaves nothing on the
/// stack, so the two paths meet at different heights and the JVM rejects the
/// method ("Inconsistent stackmap frames"). nsc materialises `()` there; so do
/// we.
pub(crate) fn pad_unit_branch(asm: &mut Assembler, branch: &Tree, result_ty: &Type) {
    if !branch.is_empty() && !is_unit_like(&branch.ty) {
        return;
    }
    if matches!(branch.ty, Type::Nothing) {
        return;
    }
    push_unit_result(asm, result_ty);
}

/// `new p.Inner(…)` names its enclosing instance explicitly. The prefix is a
/// *term* path (a val, a `this`, a chain of them); a type or package prefix
/// (`new scala.Foo`, `new Outer.Inner` for an object `Outer`) is not one, and
/// is left to the `$outer` chain / enclosing-object lookup.
pub(crate) fn new_prefix_instance<'t>(
    ctx: &EmitCtx,
    tpt: &'t Tree,
    outer: SymbolId,
) -> Option<&'t Tree> {
    let qual = match &tpt.kind {
        TreeKind::Select { qual, .. } => qual,
        TreeKind::AppliedTypeTree { tpt, .. }
        | TreeKind::TypeApply { fun: tpt, .. }
        | TreeKind::AnnotatedTypeTree { tpt, .. } => {
            return new_prefix_instance(ctx, tpt, outer);
        }
        _ => return None,
    };
    if qual.ty.is_no_type() || qual.ty.is_error() {
        return None;
    }
    let p = ctx.st.class_sym_of(&qual.ty)?;
    if !is_owner_compatible(ctx.st, p, outer) {
        return None;
    }
    Some(qual)
}

pub(crate) fn gen_new(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    tpt: &Tree,
    args: &[Tree],
    ctor_sym: SymbolId,
) {
    if let Some(elem) = array_elem_ty(&tpt.ty) {
        if let Some(len) = args.first() {
            gen_expr(asm, frame, ctx, len);
            emit_newarray(asm, ctx, &elem);
            return;
        }
    }
    let class_id = tpt
        .sym
        .is_none()
        .then(|| ctx.st.class_sym_of(&tpt.ty))
        .flatten()
        .or(if tpt.sym.is_none() {
            None
        } else {
            Some(tpt.sym)
        })
        .or_else(|| ctx.st.class_sym_of(&tpt.ty))
        .unwrap_or(tpt.sym);
    let internal = if class_id.is_none() {
        tpt.name().unwrap_or("java/lang/Object").to_string()
    } else {
        class_internal(ctx.st, class_id)
    };
    let desc = if !ctor_sym.is_none() && ctx.st.get(ctor_sym).name == "<init>" {
        method_desc_from_sym(ctx.st, ctor_sym)
    } else if class_id.is_none() {
        let pts: Vec<Type> = args.iter().map(|a| a.ty.clone()).collect();
        jvm_method_desc(ctx.st, &pts, &Type::Unit)
    } else {
        ctor_desc(ctx.st, class_id, args)
    };
    let desc = if class_id.is_none() {
        desc
    } else {
        desc_with_extra_params(
            &with_enclosing_outer_param(ctx.st, class_id, &desc),
            &capture_params_desc(ctx.st, ctx.boxed_vars, class_id),
        )
    };
    let field_tys: Vec<Type> = ctor_param_tys(ctx.st, ctor_sym, class_id, args);
    // `new ArrayDeque[Int]()` / `new Queue[Int]()` / `new Stack[Int]()`:
    // 2.13 declares these as `class Queue[A](initialSize: Int =
    // ArrayDeque.DefaultInitialSize)`, so there is no `<init>()V` to call --
    // the empty argument list means "take the default". Emitting `()V`
    // compiled and then died with `NoSuchMethodError` at run time; nsc calls
    // the synthetic default getter, and so do we.
    if args.is_empty() && has_default_sized_ctor(&internal) {
        asm.new_obj(&internal);
        asm.dup();
        asm.invokestatic(&internal, "$lessinit$greater$default$1", "()I");
        asm.invokespecial(&internal, "<init>", "(I)V");
        return;
    }
    asm.new_obj(&internal);
    asm.dup();
    if let Some(outer) = outer_field_class(ctx.st, class_id) {
        match new_prefix_instance(ctx, tpt, outer) {
            // `new i.Deep()` / `new c.Inner`: the enclosing instance is the
            // prefix that was written, not the current `this`.
            // `new_prefix_instance` already checked the prefix conforms.
            Some(pfx) => gen_expr(asm, frame, ctx, pfx),
            None => load_outer_arg(asm, ctx, outer),
        }
    }
    // A repeated constructor parameter (`class C(xs: T*)`) is one `Seq`
    // argument on the JVM, not one per element: `new SetTupleParameter(c1,
    // c2)` has to wrap them. The descriptor already says `Seq` (`jvm_desc` of
    // `Type::Repeated`), so emitting the elements raw was a `VerifyError`.
    if field_tys.iter().any(|p| matches!(p, Type::Repeated(_))) {
        let java_varargs = !ctor_sym.is_none() && {
            let f = ctx.st.get(ctor_sym).flags;
            f.contains(Flags::JAVA) && f.contains(Flags::VARARGS)
        };
        gen_call_args(
            asm,
            frame,
            ctx,
            args,
            &field_tys,
            ctx.library_abi,
            java_varargs,
            ctor_sym,
        );
    } else {
        for (i, a) in args.iter().enumerate() {
            gen_expr(asm, frame, ctx, a);
            let pty = field_tys.get(i).unwrap_or(&a.ty);
            adapt_unit_arg(asm, ctx, a, pty);
            if is_jvm_primitive(&a.ty) && !is_unit_like(&a.ty) && !is_jvm_primitive(pty) {
                emit_box(asm, &a.ty);
            }
        }
    }
    for id in class_captures(ctx.st, class_id).to_vec() {
        load_capture_arg(asm, frame, ctx, id);
    }
    asm.invokespecial(&internal, "<init>", &desc);
}

pub(crate) fn gen_apply(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    tree: &Tree,
    fun: &Tree,
    args: &[Tree],
) {
    // `f.asInstanceOf[A => B](v)`: the cast yields a *value*, and the
    // arguments belong to that value's `apply`. `peel_fun` below strips the
    // `TypeApply` and would call `asInstanceOf` itself -- with an argument it
    // does not take (`NoSuchMethodError: java.lang.Object.asInstanceOf()`, in
    // slick's `BasicBackend`:
    // `f.asInstanceOf[Any => DBIOAction[?, Streaming[T], Nothing]](v)`).
    // Anything else applied to a cast goes through an explicit `apply`
    // selection the typer inserts, so this shape means a function value.
    if let TreeKind::TypeApply { fun: head, .. } = &fun.kind {
        if matches!(head.kind, TreeKind::Select { .. })
            && !head.sym.is_none()
            && matches!(ctx.st.get(head.sym).intrinsic, Intrinsic::AsInstanceOf)
        {
            gen_function_apply(asm, frame, ctx, fun, args, &tree.ty);
            return;
        }
    }

    let fun0 = peel_fun(fun);
    let (fun, owned_args) = flatten_apply_owned(fun0, args);
    let args: &[Tree] = &owned_args;

    if let TreeKind::Select { qual, name } = &fun.kind {
        if matches!(qual.ty, Type::Refined { .. }) && fun.sym.is_none() {
            gen_structural_call(asm, frame, ctx, qual, name, args, &tree.ty);
            return;
        }
    }

    if matches!(&fun.kind, TreeKind::New { .. }) {
        let tpt = match &fun.kind {
            TreeKind::New { tpt } => tpt,
            _ => unreachable!(),
        };
        gen_new(asm, frame, ctx, tpt, args, tree.sym);
        return;
    }

    let ic = if !fun.sym.is_none() {
        ctx.st.get(fun.sym).intrinsic
    } else {
        Intrinsic::None
    };

    if ctx.library_abi && (matches!(ic, Intrinsic::Println) || fun.name() == Some("println")) {
        gen_predef_println(asm, frame, ctx, args, true);
        return;
    }
    if ctx.library_abi && (matches!(ic, Intrinsic::Print) || fun.name() == Some("print")) {
        gen_predef_println(asm, frame, ctx, args, false);
        return;
    }

    if matches!(ic, Intrinsic::Println) || fun.name() == Some("println") {
        gen_println(asm, frame, ctx, args, true);
        return;
    }
    if matches!(ic, Intrinsic::Print) || fun.name() == Some("print") {
        gen_println(asm, frame, ctx, args, false);
        return;
    }

    if fun.name() == Some("$box") {
        if let Some(a) = args.first() {
            gen_expr(asm, frame, ctx, a);
            if is_unit_like(&a.ty) {
                // nsc boxes Unit as BoxedUnit.UNIT. A call erased through
                // `Object` (`ArrayOps.head`, `def id[A](a: A): A`) already left
                // that ref on the stack; a Unit literal left nothing.
                if !unit_leaves_boxed_ref(a, ctx.st) {
                    emit_boxed_unit(asm);
                }
            } else {
                emit_box(asm, &a.ty);
            }
        } else {
            asm.aconst_null();
        }
        return;
    }
    if fun.name() == Some("$unbox") {
        if let Some(a) = args.first() {
            gen_expr(asm, frame, ctx, a);
            emit_unbox(asm, &tree.ty);
        } else {
            push_default(asm, &tree.ty);
        }
        return;
    }
    // A user value class boxes as a real instance of itself, not as the box of
    // its underlying type: `new Meters(n)` / `((Meters) x).n()`. nsc's
    // post-erasure `box`/`unbox` for `class C(val u: U) extends AnyVal`.
    if fun.name() == Some("$vcbox") {
        let cls = fun.sym;
        let internal = class_internal(ctx.st, cls);
        let under = ctx
            .st
            .get(cls)
            .ctor_fields
            .first()
            .map(|f| ctx.st.get(*f).ty.clone())
            .unwrap_or(Type::Any);
        asm.new_obj(&internal);
        asm.dup();
        if let Some(a) = args.first() {
            gen_expr(asm, frame, ctx, a);
        } else {
            push_default(asm, &under);
        }
        asm.invokespecial(
            &internal,
            "<init>",
            &format!("({})V", jvm_desc(ctx.st, &under)),
        );
        return;
    }
    if fun.name() == Some("$vcunbox") {
        let cls = fun.sym;
        let internal = class_internal(ctx.st, cls);
        let field = ctx.st.get(cls).ctor_fields.first().copied();
        let (name, under) = match field {
            Some(f) => (ctx.st.get(f).name.clone(), ctx.st.get(f).ty.clone()),
            None => (String::new(), Type::Any),
        };
        if let Some(a) = args.first() {
            gen_expr(asm, frame, ctx, a);
        } else {
            asm.aconst_null();
        }
        asm.checkcast(&internal);
        asm.invokevirtual(&internal, &name, &format!("(){}", jvm_desc(ctx.st, &under)));
        return;
    }

    if ctx.library_abi && matches!(ic, Intrinsic::Assert) {
        gen_predef_assert_require(asm, frame, ctx, args, true);
        return;
    }
    if ctx.library_abi && matches!(ic, Intrinsic::Require) {
        gen_predef_assert_require(asm, frame, ctx, args, false);
        return;
    }
    if ctx.library_abi && matches!(ic, Intrinsic::NotImplemented) {
        emit_predef_nyi(asm);
        // `Predef.???` is declared to return `Nothing$` but always throws.
        // Drop the phantom slot so a `Unit` statement (e.g. `try ???`) does
        // not leave a value under the catch handler. `Nothing` itself is
        // handled generically by `gen_expr`'s `athrow`-append, which is what
        // nsc actually emits here (`javap`: `invokevirtual ???; athrow`) --
        // popping it too would empty the stack out from under that `athrow`.
        if is_unit_like(&tree.ty) {
            asm.pop();
        }
        return;
    }

    if matches!(ic, Intrinsic::Assert) {
        gen_assert_require(asm, frame, ctx, args, true);
        return;
    }
    if matches!(ic, Intrinsic::Require) {
        gen_assert_require(asm, frame, ctx, args, false);
        return;
    }
    if matches!(ic, Intrinsic::NotImplemented) {
        throw_not_implemented(asm);
        push_default(asm, &tree.ty);
        return;
    }
    if ctx.library_abi
        && (fun.name() == Some("identity")
            || fun.name() == Some("locally")
            || fun.name() == Some("implicitly")
            || matches!(
                ic,
                Intrinsic::Identity | Intrinsic::Locally | Intrinsic::Implicitly
            ) && fun
                .name()
                .is_some_and(|n| n == "identity" || n == "locally" || n == "implicitly"))
    {
        gen_predef_poly(
            asm,
            frame,
            ctx,
            args,
            &tree.ty,
            fun.name().unwrap_or("identity"),
        );
        return;
    }

    if matches!(ic, Intrinsic::Identity) {
        if let Some(a) = args.first() {
            gen_expr(asm, frame, ctx, a);
        } else {
            gen_receiver(asm, frame, ctx, fun);
        }
        return;
    }
    if matches!(ic, Intrinsic::Implicitly) {
        if let Some(a) = args.first() {
            gen_expr(asm, frame, ctx, a);
        } else {
            push_default(asm, &tree.ty);
        }
        return;
    }
    if matches!(ic, Intrinsic::Locally) {
        if let Some(a) = args.first() {
            gen_expr(asm, frame, ctx, a);
            if matches!(&a.ty, Type::Function { .. }) {
                asm.invokeinterface("scala/Function0", "apply", "()Ljava/lang/Object;");
                if is_unit_like(&tree.ty) {
                    asm.pop();
                } else if is_jvm_primitive(&tree.ty) {
                    emit_unbox(asm, &tree.ty);
                } else if matches!(tree.ty, Type::String) {
                    asm.checkcast("java/lang/String");
                }
            }
        } else {
            push_default(asm, &tree.ty);
        }
        return;
    }
    if let Intrinsic::BoxValue(desc) = ic {
        // nsc's `Predef.int2Integer` is `Integer.valueOf` and nothing else, so
        // emitting the wrapper call directly is faithful *and* works on the
        // private runtime, which has no `scala/Predef$.int2Integer`.
        let prim = prim_of_desc(desc);
        if let Some(a) = args.first() {
            gen_expr(asm, frame, ctx, a);
            widen_primitive(asm, &a.ty, &prim);
        } else {
            push_default(asm, &prim);
        }
        emit_box(asm, &prim);
        return;
    }
    if let Intrinsic::UnboxValue(desc) = ic {
        let prim = prim_of_desc(desc);
        if let Some(a) = args.first() {
            gen_expr(asm, frame, ctx, a);
        } else {
            asm.aconst_null();
        }
        emit_unbox(asm, &prim);
        return;
    }
    if matches!(ic, Intrinsic::Any2StringAdd) {
        if let Some(a) = args.first() {
            gen_expr(asm, frame, ctx, a);
        } else {
            push_default(asm, &tree.ty);
        }
        return;
    }
    if matches!(ic, Intrinsic::WrapArrowAssoc) {
        // Identity: `->` is lowered to `new Tuple2` so ArrowAssoc is never allocated.
        if let Some(a) = args.first() {
            gen_expr(asm, frame, ctx, a);
            if is_jvm_primitive(&a.ty) {
                emit_box(asm, &a.ty);
            }
        } else {
            asm.aconst_null();
        }
        return;
    }
    if matches!(ic, Intrinsic::StringToInt) {
        gen_receiver(asm, frame, ctx, fun);
        asm.invokestatic("java/lang/Integer", "parseInt", "(Ljava/lang/String;)I");
        return;
    }
    if matches!(ic, Intrinsic::StringToLong) {
        gen_receiver(asm, frame, ctx, fun);
        asm.invokestatic("java/lang/Long", "parseLong", "(Ljava/lang/String;)J");
        return;
    }
    if matches!(ic, Intrinsic::StringToDouble) {
        gen_receiver(asm, frame, ctx, fun);
        asm.invokestatic("java/lang/Double", "parseDouble", "(Ljava/lang/String;)D");
        return;
    }
    if matches!(ic, Intrinsic::Eq | Intrinsic::Ne) {
        gen_eq_ne(asm, frame, ctx, fun, args, matches!(ic, Intrinsic::Eq));
        return;
    }
    if matches!(ic, Intrinsic::AnyEq | Intrinsic::AnyNe) {
        gen_any_eq(asm, frame, ctx, fun, args, matches!(ic, Intrinsic::AnyEq));
        return;
    }
    if let Intrinsic::NewTuple(n) = ic {
        // nsc lowers a tuple literal to `new TupleN`, so no `TupleN$` module
        // classfile is needed.
        let cls = format!("scala/Tuple{n}");
        asm.new_obj(&cls);
        asm.dup();
        for a in args.iter() {
            gen_expr(asm, frame, ctx, a);
            if is_jvm_primitive(&a.ty) {
                emit_box(asm, &a.ty);
            }
        }
        let desc = format!("({})V", "Ljava/lang/Object;".repeat(n));
        asm.invokespecial(&cls, "<init>", &desc);
        return;
    }
    if matches!(ic, Intrinsic::NewWrapper) {
        // `5.seconds` is `new package$DurationInt(5).seconds()` in scalac's
        // own output: the conversion erases to the identity on the underlying
        // primitive, and the unit methods live on the boxed class.
        let s = ctx.st.get(fun.sym);
        let (param, ret) = match &s.ty {
            Type::Method { paramss, ret } => (
                paramss
                    .iter()
                    .flatten()
                    .next()
                    .cloned()
                    .unwrap_or(Type::Int),
                (**ret).clone(),
            ),
            _ => (Type::Int, tree.ty.clone()),
        };
        let cls = ctx
            .st
            .class_sym_of(&ret)
            .map(|c| class_internal(ctx.st, c))
            .unwrap_or_default();
        asm.new_obj(&cls);
        asm.dup();
        if let Some(a) = args.first() {
            gen_expr(asm, frame, ctx, a);
            widen_primitive(asm, &a.ty, &param);
        } else {
            push_default(asm, &param);
        }
        asm.invokespecial(&cls, "<init>", &format!("({})V", jvm_desc(ctx.st, &param)));
        return;
    }
    if matches!(ic, Intrinsic::Synchronized) {
        gen_synchronized(asm, frame, ctx, fun, args, &tree.ty);
        return;
    }

    if matches!(&fun.ty, Type::Function { .. })
        || (fun.sym.is_none()
            && matches!(&tree.kind, TreeKind::Apply { .. })
            && matches!(&fun.ty, Type::Function { .. }))
    {
        gen_function_apply(asm, frame, ctx, fun, args, &tree.ty);
        return;
    }

    if !ctx.library_abi && is_arrow_assoc_arrow(ctx, fun) {
        gen_tuple2_arrow(asm, frame, ctx, fun, args);
        return;
    }

    if let TreeKind::Select { qual, name } = &fun.kind {
        match ic {
            Intrinsic::IntBin(op) => {
                gen_expr(asm, frame, ctx, qual);
                if let Some(r) = args.first() {
                    gen_expr(asm, frame, ctx, r);
                    // `ishl` takes an `int` shift count even though nsc also
                    // declares `Int.<<(x: Long): Int`.
                    if matches!(op, "<<" | ">>" | ">>>")
                        && matches!(r.ty.widen_constant(), Type::Long)
                    {
                        asm.l2i();
                    }
                } else {
                    asm.iconst(0);
                }
                emit_int_bin(asm, op);
                return;
            }
            Intrinsic::IntUn(op) => {
                gen_expr(asm, frame, ctx, qual);
                match op {
                    "-" => asm.ineg(),
                    "~" => {
                        asm.iconst(-1);
                        asm.ixor();
                    }
                    _ => {}
                }
                return;
            }
            Intrinsic::LongBin(op) => {
                gen_expr(asm, frame, ctx, qual);
                widen_numeric(asm, &qual.ty, &Type::Long);
                if let Some(r) = args.first() {
                    gen_expr(asm, frame, ctx, r);
                    if matches!(op, "<<" | ">>" | ">>>") {
                        // A shift count is an `int` on the JVM, whatever
                        // Scala's `<<(x: Long)` overload says.
                        if matches!(r.ty.widen_constant(), Type::Long) {
                            asm.l2i();
                        }
                    } else {
                        widen_numeric(asm, &r.ty, &Type::Long);
                    }
                }
                emit_long_bin(asm, op);
                return;
            }
            Intrinsic::LongUn("-") => {
                gen_expr(asm, frame, ctx, qual);
                asm.lneg();
                return;
            }
            Intrinsic::LongUn("~") => {
                gen_expr(asm, frame, ctx, qual);
                asm.lconst(-1);
                asm.lxor();
                return;
            }
            Intrinsic::DoubleBin(op) => {
                gen_expr(asm, frame, ctx, qual);
                widen_numeric(asm, &qual.ty, &Type::Double);
                if let Some(r) = args.first() {
                    gen_expr(asm, frame, ctx, r);
                    widen_numeric(asm, &r.ty, &Type::Double);
                }
                emit_double_bin(asm, op);
                return;
            }
            Intrinsic::DoubleUn("-") => {
                gen_expr(asm, frame, ctx, qual);
                asm.dneg();
                return;
            }
            Intrinsic::FloatBin(op) => {
                gen_expr(asm, frame, ctx, qual);
                widen_numeric(asm, &qual.ty, &Type::Float);
                if let Some(r) = args.first() {
                    gen_expr(asm, frame, ctx, r);
                    widen_numeric(asm, &r.ty, &Type::Float);
                }
                emit_float_bin(asm, op);
                return;
            }
            Intrinsic::FloatUn("-") => {
                gen_expr(asm, frame, ctx, qual);
                asm.fneg();
                return;
            }
            Intrinsic::BoolBin("&&") => {
                gen_bool_and(asm, frame, ctx, qual, args.first());
                return;
            }
            Intrinsic::BoolBin("||") => {
                gen_bool_or(asm, frame, ctx, qual, args.first());
                return;
            }
            Intrinsic::BoolBin(op) => {
                gen_expr(asm, frame, ctx, qual);
                if let Some(r) = args.first() {
                    gen_expr(asm, frame, ctx, r);
                }
                emit_int_cmp(asm, op);
                return;
            }
            Intrinsic::BoolUn("!") => {
                gen_expr(asm, frame, ctx, qual);
                asm.iconst(1);
                asm.ixor();
                return;
            }
            Intrinsic::StringConcat => {
                if let Some(r) = args.first() {
                    gen_string_concat(asm, frame, ctx, qual, r);
                } else {
                    gen_expr(asm, frame, ctx, qual);
                }
                return;
            }
            Intrinsic::AnyToString => {
                gen_expr(asm, frame, ctx, qual);
                // `().toString` is `BoxedUnit.UNIT.toString`, i.e. `"()"`.
                adapt_unit_arg(asm, ctx, qual, &qual.ty);
                if is_jvm_primitive(&qual.ty) && !is_unit_like(&qual.ty) {
                    emit_box(asm, &qual.ty);
                }
                asm.invokevirtual("java/lang/Object", "toString", "()Ljava/lang/String;");
                return;
            }
            Intrinsic::StringFormat => {
                // `String.format(fmt, args)`: the receiver is the format.
                gen_expr(asm, frame, ctx, qual);
                emit_format_args(asm, frame, ctx, args);
                asm.invokestatic(
                    "java/lang/String",
                    "format",
                    "(Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/String;",
                );
                return;
            }
            Intrinsic::AnyHash => {
                gen_expr(asm, frame, ctx, qual);
                // `().##` hashes the `BoxedUnit` singleton (`0`).
                adapt_unit_arg(asm, ctx, qual, &qual.ty);
                emit_any_hash(asm, &qual.ty);
                return;
            }
            Intrinsic::GetClass => {
                gen_expr(asm, frame, ctx, qual);
                adapt_unit_arg(asm, ctx, qual, &qual.ty);
                emit_get_class(asm, ctx, &qual.ty);
                return;
            }
            Intrinsic::Identity => {
                gen_expr(asm, frame, ctx, qual);
                return;
            }
            Intrinsic::IntToByte => {
                gen_expr(asm, frame, ctx, qual);
                asm.i2b();
                return;
            }
            Intrinsic::IntToShort => {
                gen_expr(asm, frame, ctx, qual);
                asm.i2s();
                return;
            }
            Intrinsic::IntToLong => {
                gen_expr(asm, frame, ctx, qual);
                asm.i2l();
                return;
            }
            Intrinsic::IntToDouble => {
                gen_expr(asm, frame, ctx, qual);
                asm.i2d();
                return;
            }
            Intrinsic::IntToFloat => {
                gen_expr(asm, frame, ctx, qual);
                asm.i2f();
                return;
            }
            Intrinsic::LongToFloat => {
                gen_expr(asm, frame, ctx, qual);
                asm.l2f();
                return;
            }
            Intrinsic::FloatToDouble => {
                gen_expr(asm, frame, ctx, qual);
                asm.f2d();
                return;
            }
            Intrinsic::LongToDouble => {
                gen_expr(asm, frame, ctx, qual);
                asm.l2d();
                return;
            }
            Intrinsic::NumConv(code) => {
                gen_expr(asm, frame, ctx, qual);
                emit_num_conv(asm, code);
                return;
            }
            _ => {}
        }

        if !ctx.library_abi && name == "+" && matches!(tree.ty, Type::String) {
            if let Some(r) = args.first() {
                gen_string_concat(asm, frame, ctx, qual, r);
                return;
            }
        }

        // name-based int ops if typer did not attach an intrinsic
        if args.len() == 1
            && matches!(qual.ty.widen_constant(), Type::Int)
            && matches!(args[0].ty.widen_constant(), Type::Int)
        {
            if matches!(
                name.as_str(),
                "+" | "-" | "*" | "/" | "%" | "==" | "!=" | "<" | "<=" | ">" | ">="
            ) {
                gen_expr(asm, frame, ctx, qual);
                gen_expr(asm, frame, ctx, &args[0]);
                emit_int_bin(asm, name);
                return;
            }
        }
    }

    // Function value apply (erased to FunctionN.apply)
    if matches!(&fun.ty, Type::Function { .. }) {
        gen_function_apply(asm, frame, ctx, fun, args, &tree.ty);
        return;
    }
    // A value whose *class* inherits `FunctionN` is applied the same way: the
    // only thing a non-method can mean in call position is its `apply`. An
    // implicit `ev: P <:< Q` used as a view is such a value
    // (`sealed abstract class <:<[-From, +To] extends (From => To)`), and
    // falling through to `invoke_method` emitted a call to a member of the
    // enclosing *method* -- `NoClassDefFoundError: direct` from a program that
    // typechecked.
    if !fun.sym.is_none()
        && ctx.st.get(fun.sym).kind != SymKind::Method
        && inherited_function_arity(ctx.st, &fun.ty) == Some(args.len())
    {
        gen_function_apply(asm, frame, ctx, fun, args, &tree.ty);
        return;
    }

    // regular method / apply
    if fun.sym.is_none() {
        throw_runtime(asm, "unresolved apply");
        push_default(asm, &tree.ty);
        return;
    }

    // `a(i)` / `a(i) = x` / `a.clone()` where the receiver is an `Array[T]`
    // whose element type is abstract. Such an array erases to `Object`, so
    // there is no `aaload` / `aastore` to emit and no class to name in a
    // `Methodref`: nsc calls `ScalaRunTime.array_apply` / `array_update` /
    // `array_clone`, the same detour `length` already takes here. Without this
    // the call went out as `invokevirtual "[java/lang/Object".update` — a name
    // the JVM rejects outright, so both a `def repeat[T: ClassTag](x: T, n:
    // Int)` filling a `new Array[T](n)` and a `def dup[T](a: Array[T]) =
    // a.clone()` produced a class file that would not even load
    // (`ClassFormatError: Illegal class name`).
    //
    // The test is on the receiver's *type*, not on the element: by this point
    // an abstract-element array has been erased and no longer arrives as a
    // `Type::Array` at all, which is exactly why the array paths below missed
    // it.
    if let TreeKind::Select { qual, name } = &fun.kind {
        let want = match name.as_str() {
            "apply" => Some(1),
            "update" => Some(2),
            "clone" => Some(0),
            _ => None,
        };
        if ctx.st.get(fun.sym).owner == ctx.st.array_sym
            && want == Some(args.len())
            && !matches!(qual.ty, Type::Array(_))
        {
            if !ctx.library_abi {
                throw_runtime(
                    asm,
                    "generic Array element access needs the scala-library ClassTag runtime",
                );
                push_default(asm, &tree.ty);
                return;
            }
            asm.getstatic(
                "scala/runtime/ScalaRunTime$",
                "MODULE$",
                "Lscala/runtime/ScalaRunTime$;",
            );
            gen_expr(asm, frame, ctx, qual);
            let ptys: &[Type] = match name.as_str() {
                "apply" => &[Type::Int],
                "update" => &[Type::Int, Type::Any],
                _ => &[],
            };
            gen_call_args(asm, frame, ctx, args, ptys, true, false, SymbolId::NONE);
            match name.as_str() {
                "apply" => {
                    let d = "(Ljava/lang/Object;I)Ljava/lang/Object;";
                    asm.invokevirtual("scala/runtime/ScalaRunTime$", "array_apply", d);
                    maybe_unbox_erased_result(asm, ctx, d, Some(&tree.ty));
                }
                "update" => asm.invokevirtual(
                    "scala/runtime/ScalaRunTime$",
                    "array_update",
                    "(Ljava/lang/Object;ILjava/lang/Object;)V",
                ),
                _ => asm.invokevirtual(
                    "scala/runtime/ScalaRunTime$",
                    "array_clone",
                    "(Ljava/lang/Object;)Ljava/lang/Object;",
                ),
            }
            return;
        }
    }
    // An `$extension` that lives on a companion module takes the receiver as
    // its first *argument*, so the module has to be on the stack under it --
    // and under the arguments that follow. It goes down first, before the
    // receiver is even evaluated, which is what nsc emits too. Pushing it
    // afterwards and shuffling only ever worked for a single argument.
    let ext_module_pushed = !fun.sym.is_none()
        && ctx.st.is_value_class(ctx.st.get(fun.sym).owner)
        && match value_extension_module(ctx.st, fun.sym) {
            Some(m) => {
                asm.getstatic(&m, "MODULE$", &format!("L{m};"));
                true
            }
            None => false,
        };
    // A value class calling another of its own methods reaches it as an
    // `$extension` static, whose first argument is the *underlying* value --
    // and `this` is the box. `gen_value_self_receiver` is the one place the
    // two representations meet; everywhere else a value-class-typed expression
    // has already been erased to its underlying.
    if !gen_value_self_receiver(asm, ctx, fun) {
        gen_receiver(asm, frame, ctx, fun);
    }
    if let TreeKind::Select { qual, .. } = &fun.kind {
        // `ArrayOps.toList` / `toSet` / `toVector` / `toBuffer` / `sum` /
        // `product` / `min` / `max` / `minBy` / `maxBy` / `mkString` /
        // `reduce` / `reduceLeft` do not exist on `ArrayOps` itself in nsc
        // 2.13.16 (confirmed via `javap -s scala.collection.ArrayOps`); nsc
        // reaches them by widening the array to
        // `scala.collection.mutable.ArraySeq` (`Predef.wrapXArray`) and
        // calling the `IterableOnceOps` default method. Do the same right
        // after the receiver (the raw array) lands on the stack, before any
        // further (possibly implicit) arguments are pushed.
        if !fun.sym.is_none() && is_array_ops_wrap_method(ctx.st, fun.sym) {
            emit_array_wrap_to_iterable_ops(asm, ctx);
        }
        checkcast_refined_receiver(asm, ctx, &qual.ty, fun.sym);
    }
    checkcast_erased_method_receiver(asm, ctx, fun);
    let value_owner = if !fun.sym.is_none() && ctx.st.is_value_class(ctx.st.get(fun.sym).owner) {
        Some(ctx.st.get(fun.sym).owner)
    } else {
        None
    };
    if let Some(owner) = value_owner {
        if let TreeKind::Select { qual, .. } = &fun.kind {
            box_value_class_receiver(asm, ctx, owner, qual);
        }
    }
    let param_tys: Vec<Type> = if !fun.sym.is_none() {
        match &ctx.st.get(fun.sym).ty {
            Type::Method { paramss, .. } => paramss.iter().flatten().cloned().collect(),
            Type::Function { params, .. } => params.clone(),
            _ => Vec::new(),
        }
    } else {
        Vec::new()
    };
    let java_varargs = !fun.sym.is_none() && {
        let f = ctx.st.get(fun.sym).flags;
        f.contains(Flags::JAVA) && f.contains(Flags::VARARGS)
    };
    let array_elem_op = matches!(
        &fun.kind,
        TreeKind::Select { qual, name }
            if (name == "update" || name == "apply") && matches!(qual.ty, Type::Array(_))
    );
    // `this(...)` inside an auxiliary constructor hands the primary one the
    // enclosing instance it was itself given (slot 1), ahead of the arguments.
    if !fun.sym.is_none()
        && ctx.st.get(fun.sym).name == "<init>"
        && outer_field_class(ctx.st, ctx.st.get(fun.sym).owner).is_some()
    {
        asm.aload(1);
    }
    gen_call_args(
        asm,
        frame,
        ctx,
        args,
        &param_tys,
        value_owner.is_some() || (ctx.library_abi && !array_elem_op),
        java_varargs,
        fun.sym,
    );
    if let TreeKind::Select { qual, name } = &fun.kind {
        if name == "apply" && matches!(qual.ty, Type::Array(_)) {
            let elem = match &qual.ty {
                Type::Array(e) => e.as_ref(),
                _ => &tree.ty,
            };
            emit_array_load(asm, elem);
            if matches!(elem, Type::String) {
                asm.checkcast("java/lang/String");
            }
            return;
        }
        if name == "update" && matches!(qual.ty, Type::Array(_)) {
            emit_array_store(asm, &qual.ty);
            return;
        }
        if name == "clone"
            && matches!(qual.ty, Type::Array(_))
            && !fun.sym.is_none()
            && ctx.st.get(fun.sym).owner == ctx.st.array_sym
        {
            // The class a JVM array's `clone()` is named on is the array's own
            // *descriptor* (`"[I".clone:()Ljava/lang/Object;`), which is what
            // nsc emits, plus the `checkcast` back. An abstract element type
            // makes the whole array erase to `Object`, and `jvm_desc` would
            // still spell it `[Ljava/lang/Object;` -- the wrong class to name
            // for what may be an `int[]` -- so that case goes through
            // `ScalaRunTime.array_clone` (the block above catches it before
            // the receiver is pushed; this arm is the belt to that braces).
            let concrete = matches!(&qual.ty, Type::Array(e) if is_concrete_array_elem(e));
            let desc = jvm_desc(ctx.st, &qual.ty);
            if concrete {
                asm.invokevirtual(&desc, "clone", "()Ljava/lang/Object;");
                asm.checkcast(&desc);
            } else if ctx.library_abi {
                asm.getstatic(
                    "scala/runtime/ScalaRunTime$",
                    "MODULE$",
                    "Lscala/runtime/ScalaRunTime$;",
                );
                asm.swap();
                asm.invokevirtual(
                    "scala/runtime/ScalaRunTime$",
                    "array_clone",
                    "(Ljava/lang/Object;)Ljava/lang/Object;",
                );
            } else {
                throw_runtime(
                    asm,
                    "Array[T].clone needs the scala-library ScalaRunTime.array_clone",
                );
                push_default(asm, &tree.ty);
            }
            return;
        }
    }
    if fun_is_super(fun) {
        invoke_super(asm, ctx, fun.sym);
    } else if value_owner.is_some() {
        invoke_value_extension(asm, ctx, fun.sym, Some(&tree.ty), ext_module_pushed);
    } else {
        invoke_method(asm, ctx, fun.sym, Some(&tree.ty));
    }
}

pub(crate) fn fun_is_super(fun: &Tree) -> bool {
    match &peel_fun(fun).kind {
        TreeKind::Select { qual, .. } => matches!(qual.kind, TreeKind::Super { .. }),
        _ => false,
    }
}

pub(crate) fn invoke_super(asm: &mut Assembler, ctx: &EmitCtx, id: SymbolId) {
    let s = ctx.st.get(id);
    let desc = method_desc_from_sym(ctx.st, id);
    if is_interface_sym(ctx.st, ctx.class_sym) {
        let acc = super_accessor_name(ctx.st, ctx.class_sym, &s.name);
        let iface = class_internal(ctx.st, ctx.class_sym);
        // The `T$$super$m` accessor is a member of *this* trait, declared and
        // implemented with the overriding method's own erasure -- which is not
        // always the parent's. `type RowsPerStatement = One.type` narrows an
        // inherited `insertAll(Iterable, RowsPerStatement)` from `Rps` to
        // `Rps$One$`; calling the accessor at the parent's descriptor found no
        // such method.
        let acc_desc = if !ctx.method_sym.is_none() && ctx.st.get(ctx.method_sym).name == s.name {
            method_desc_from_sym(ctx.st, ctx.method_sym)
        } else {
            desc
        };
        asm.invokeinterface(&iface, &acc, &acc_desc);
        return;
    }
    let owner_id = s.owner;
    let owner = class_internal(ctx.st, owner_id);
    if is_interface_sym(ctx.st, owner_id) {
        let static_desc = trait_static_desc(&owner, &desc);
        asm.invokestatic_interface(&owner, &trait_static_name(&s.name), &static_desc);
    } else {
        asm.invokespecial(&owner, &s.name, &desc);
    }
}

/// `ArrayOps` members with no `$extension` static and no direct instance
/// method in nsc 2.13.16 (checked via `javap -s scala.collection.ArrayOps`).
/// Reached at runtime through `scala.LowPriorityImplicits.wrapXArray` +
/// `scala.collection.IterableOnceOps`'s default methods.
pub(crate) const ARRAY_OPS_WRAP_METHODS: &[&str] = &[
    "toList",
    "toSet",
    "toVector",
    "toBuffer",
    "mkString",
    "reduce",
    "reduceLeft",
    "sum",
    "product",
    "min",
    "max",
    "minBy",
    "maxBy",
];

pub(crate) fn param_count(st: &SymbolTable, id: SymbolId) -> usize {
    match &st.get(id).ty {
        Type::Method { paramss, .. } => paramss.iter().map(|c| c.len()).sum(),
        _ => 0,
    }
}

pub(crate) fn is_array_ops_wrap_method(st: &SymbolTable, id: SymbolId) -> bool {
    if id.is_none() {
        return false;
    }
    let s = st.get(id);
    class_internal(st, s.owner) == "scala/collection/ArrayOps"
        && ARRAY_OPS_WRAP_METHODS.contains(&s.name.as_str())
}

/// Turn the raw array on top of the stack into a
/// `scala.collection.mutable.ArraySeq` via
/// `scala.Predef$.MODULE$.genericWrapArray` (`scala.LowPriorityImplicits`,
/// mixed into the `Predef` object as a real superclass — not a Scala 2.12+
/// default-method interface — so a plain `invokevirtual` resolves it).
/// `genericWrapArray` inspects the array's own runtime class
/// (`ArraySeq.make`) to build the right primitive/ref `ArraySeq` subclass, so
/// it is correct for any element type without the backend having to track
/// it through erasure (`qual.ty` is `Any` by the time this call is
/// generated — the element type was already erased).
pub(crate) fn emit_array_wrap_to_iterable_ops(asm: &mut Assembler, ctx: &EmitCtx) {
    let _ = ctx;
    // stack: [arrayRef] -> [arrayRef, Predef$] -> [Predef$, arrayRef] -> [wrapped]
    asm.getstatic("scala/Predef$", "MODULE$", "Lscala/Predef$;");
    asm.swap();
    asm.invokevirtual(
        "scala/Predef$",
        "genericWrapArray",
        "(Ljava/lang/Object;)Lscala/collection/mutable/ArraySeq;",
    );
}

/// Turn the raw array on top of the stack into a
/// `scala.collection.immutable.IndexedSeq` via
/// `scala.Predef$.MODULE$.copyArrayToImmutableIndexedSeq`
/// (`scala.LowPriorityImplicits2`, a real superclass of the `Predef` object,
/// so `invokevirtual` resolves it).
///
/// This is the wrapping a *repeated parameter* needs, and it is the one nsc
/// picks there: a repeated parameter erases to
/// `scala/collection/immutable/Seq`, which the `mutable.ArraySeq` that
/// [`emit_array_wrap_to_iterable_ops`] produces is not.
pub(crate) fn emit_array_copy_to_immutable_seq(asm: &mut Assembler) {
    // stack: [arrayRef] -> [arrayRef, Predef$] -> [Predef$, arrayRef] -> [seq]
    asm.getstatic("scala/Predef$", "MODULE$", "Lscala/Predef$;");
    asm.swap();
    asm.invokevirtual(
        "scala/Predef$",
        "copyArrayToImmutableIndexedSeq",
        "(Ljava/lang/Object;)Lscala/collection/immutable/IndexedSeq;",
    );
}

/// `module_pushed` says the caller has already pushed the companion module
/// *under* the receiver, which is the only way to reach an `$extension` that
/// takes arguments: the JVM cannot insert a value below several stack slots,
/// and nsc pushes the module first for the same reason.
pub(crate) fn invoke_value_extension(
    asm: &mut Assembler,
    ctx: &EmitCtx,
    id: SymbolId,
    result_ty: Option<&Type>,
    module_pushed: bool,
) {
    let s = ctx.st.get(id);
    let owner_id = s.owner;
    let owner = class_internal(ctx.st, owner_id);
    if owner == "scala/runtime/RichBoolean" && s.name == "compare" {
        // No compare$extension; nsc allocates RichBoolean and calls compare(Object).
        // stack: recv_z, arg_z
        asm.swap();
        asm.new_obj("scala/runtime/RichBoolean");
        asm.dup_x1();
        asm.swap();
        asm.invokespecial("scala/runtime/RichBoolean", "<init>", "(Z)V");
        asm.swap();
        asm.invokestatic("java/lang/Boolean", "valueOf", "(Z)Ljava/lang/Boolean;");
        asm.invokevirtual(
            "scala/runtime/RichBoolean",
            "compare",
            "(Ljava/lang/Object;)I",
        );
        return;
    }
    // `sign`/`round`/`floor`/`ceil` have no `$extension` static counterpart
    // on `RichInt`/`RichLong`/`RichDouble` (unlike `toBinaryString` etc.), so
    // nsc would allocate a real instance and call the instance method. Both
    // delegate to plain `java.lang`/`Math` statics under the hood, so call
    // those directly instead -- same result, and avoids the category-1 vs.
    // category-2 stack-shuffling that a `new`+dup-based call would need for
    // `Long`/`Double` receivers.
    // `3.compare(4)` -- nsc reaches `OrderedProxy.compare`, which is
    // `java.lang.Integer.compare` under the hood. There is no
    // `compare$extension`, and allocating the proxy just to call one static
    // buys nothing, so call the static directly (same result, and the
    // receiver stays unboxed).
    if s.name == "compare" {
        let prim = match owner.as_str() {
            "scala/runtime/RichByte" => Some(("java/lang/Byte", "(BB)I")),
            "scala/runtime/RichShort" => Some(("java/lang/Short", "(SS)I")),
            "scala/runtime/RichInt" => Some(("java/lang/Integer", "(II)I")),
            "scala/runtime/RichLong" => Some(("java/lang/Long", "(JJ)I")),
            "scala/runtime/RichFloat" => Some(("java/lang/Float", "(FF)I")),
            "scala/runtime/RichDouble" => Some(("java/lang/Double", "(DD)I")),
            "scala/runtime/RichChar" => Some(("java/lang/Character", "(CC)I")),
            _ => None,
        };
        if let Some((cls, desc)) = prim {
            asm.invokestatic(cls, "compare", desc);
            return;
        }
    }
    if owner == "scala/runtime/RichInt" && s.name == "sign" {
        asm.invokestatic("java/lang/Integer", "signum", "(I)I");
        return;
    }
    if owner == "scala/runtime/RichLong" && s.name == "sign" {
        asm.invokestatic("java/lang/Long", "signum", "(J)I");
        asm.i2l();
        return;
    }
    if owner == "scala/runtime/RichDouble" && s.name == "sign" {
        asm.invokestatic("java/lang/Math", "signum", "(D)D");
        return;
    }
    if owner == "scala/runtime/RichDouble" && s.name == "round" {
        asm.invokestatic("java/lang/Math", "round", "(D)J");
        return;
    }
    if owner == "scala/runtime/RichDouble" && s.name == "floor" {
        asm.invokestatic("java/lang/Math", "floor", "(D)D");
        return;
    }
    if owner == "scala/runtime/RichDouble" && s.name == "ceil" {
        asm.invokestatic("java/lang/Math", "ceil", "(D)D");
        return;
    }
    if owner == "scala/collection/StringOps" && s.name == "length" {
        // 2.13 StringOps inlines `length` to `String#length`; the jar exposes
        // `size$extension` which is the same call against the underlying String.
        asm.invokestatic(
            "scala/collection/StringOps",
            "size$extension",
            "(Ljava/lang/String;)I",
        );
        return;
    }
    if owner == "scala/collection/StringOps" && s.name == "isEmpty" {
        // Same as scalac: StringOps.isEmpty is inlined to String#isEmpty.
        asm.invokevirtual("java/lang/String", "isEmpty", "()Z");
        return;
    }
    if owner == "scala/collection/StringOps" && s.name == "toUpperCase" {
        // 2.13 StringOps inlines toUpperCase/toLowerCase to String.
        asm.invokevirtual("java/lang/String", "toUpperCase", "()Ljava/lang/String;");
        return;
    }
    if owner == "scala/collection/StringOps" && s.name == "toLowerCase" {
        asm.invokevirtual("java/lang/String", "toLowerCase", "()Ljava/lang/String;");
        return;
    }
    if owner == "scala/collection/StringOps" && s.name == "toArray" {
        asm.invokestatic(
            "scala/collection/StringOps",
            "toArray$extension",
            "(Ljava/lang/String;Lscala/reflect/ClassTag;)Ljava/lang/Object;",
        );
        maybe_unbox_erased_result(
            asm,
            ctx,
            "(Ljava/lang/Object;)Ljava/lang/Object;",
            result_ty,
        );
        return;
    }
    if owner == "scala/runtime/RichInt" && s.name == "to" {
        // 2.13 `to` returns Range$Inclusive, not the abstract Range.
        asm.invokestatic(
            "scala/runtime/RichInt",
            "to$extension",
            "(II)Lscala/collection/immutable/Range$Inclusive;",
        );
        return;
    }
    if owner == "scala/runtime/RichInt" && s.name == "until" {
        asm.invokestatic(
            "scala/runtime/RichInt",
            "until$extension",
            "(II)Lscala/collection/immutable/Range;",
        );
        return;
    }
    if owner == "scala/runtime/RichByte" && (s.name == "to" || s.name == "until") {
        // RichByte has no to$extension; IntegralProxy default builds a real NumericRange.
        emit_integral_numeric_range(asm, &Type::Byte, s.name == "to");
        return;
    }
    if owner == "scala/runtime/RichShort" && (s.name == "to" || s.name == "until") {
        emit_integral_numeric_range(asm, &Type::Short, s.name == "to");
        return;
    }
    if owner == "scala/runtime/RichLong" && (s.name == "to" || s.name == "until") {
        emit_long_numeric_range(asm, s.name == "to");
        return;
    }
    if owner == "scala/runtime/RichChar" && (s.name == "to" || s.name == "until") {
        emit_integral_numeric_range(asm, &Type::Char, s.name == "to");
        return;
    }
    if owner == "scala/runtime/RichChar" && s.name == "toInt" {
        // RichChar.toInt is inlined; the jar exposes intValue$extension.
        asm.invokestatic("scala/runtime/RichChar", "intValue$extension", "(C)I");
        return;
    }
    if owner == "scala/collection/ArrayOps" {
        // 2.13 ArrayOps is AnyVal over erased Array.
        if s.name == "foreach" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "foreach$extension",
                "(Ljava/lang/Object;Lscala/Function1;)V",
            );
            return;
        }
        if s.name == "map" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "map$extension",
                "(Ljava/lang/Object;Lscala/Function1;Lscala/reflect/ClassTag;)Ljava/lang/Object;",
            );
            maybe_unbox_erased_result(
                asm,
                ctx,
                "(Ljava/lang/Object;)Ljava/lang/Object;",
                result_ty,
            );
            return;
        }
        if s.name == "filter" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "filter$extension",
                "(Ljava/lang/Object;Lscala/Function1;)Ljava/lang/Object;",
            );
            maybe_unbox_erased_result(
                asm,
                ctx,
                "(Ljava/lang/Object;)Ljava/lang/Object;",
                result_ty,
            );
            return;
        }
        if s.name == "slice" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "slice$extension",
                "(Ljava/lang/Object;II)Ljava/lang/Object;",
            );
            maybe_unbox_erased_result(
                asm,
                ctx,
                "(Ljava/lang/Object;)Ljava/lang/Object;",
                result_ty,
            );
            return;
        }
        if s.name == "flatMap" {
            let n = match &s.ty {
                Type::Method { paramss, .. } => paramss.iter().flatten().count(),
                _ => s.params.len(),
            };
            let desc = if n >= 3 {
                "(Ljava/lang/Object;Lscala/Function1;Lscala/Function1;Lscala/reflect/ClassTag;)Ljava/lang/Object;"
            } else {
                "(Ljava/lang/Object;Lscala/Function1;Lscala/reflect/ClassTag;)Ljava/lang/Object;"
            };
            asm.invokestatic("scala/collection/ArrayOps", "flatMap$extension", desc);
            maybe_unbox_erased_result(
                asm,
                ctx,
                "(Ljava/lang/Object;)Ljava/lang/Object;",
                result_ty,
            );
            return;
        }
        if s.name == "take" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "take$extension",
                "(Ljava/lang/Object;I)Ljava/lang/Object;",
            );
            maybe_unbox_erased_result(
                asm,
                ctx,
                "(Ljava/lang/Object;)Ljava/lang/Object;",
                result_ty,
            );
            return;
        }
        if s.name == "collect" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "collect$extension",
                "(Ljava/lang/Object;Lscala/PartialFunction;Lscala/reflect/ClassTag;)Ljava/lang/Object;",
            );
            maybe_unbox_erased_result(
                asm,
                ctx,
                "(Ljava/lang/Object;)Ljava/lang/Object;",
                result_ty,
            );
            return;
        }
        if s.name == "zip" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "zip$extension",
                "(Ljava/lang/Object;Lscala/collection/IterableOnce;)[Lscala/Tuple2;",
            );
            return;
        }
        if s.name == "drop" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "drop$extension",
                "(Ljava/lang/Object;I)Ljava/lang/Object;",
            );
            maybe_unbox_erased_result(
                asm,
                ctx,
                "(Ljava/lang/Object;)Ljava/lang/Object;",
                result_ty,
            );
            return;
        }
        if s.name == "dropWhile" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "dropWhile$extension",
                "(Ljava/lang/Object;Lscala/Function1;)Ljava/lang/Object;",
            );
            maybe_unbox_erased_result(
                asm,
                ctx,
                "(Ljava/lang/Object;)Ljava/lang/Object;",
                result_ty,
            );
            return;
        }
        if s.name == "exists" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "exists$extension",
                "(Ljava/lang/Object;Lscala/Function1;)Z",
            );
            return;
        }
        if s.name == "foldLeft" || s.name == "fold" || s.name == "foldRight" {
            let desc = "(Ljava/lang/Object;Ljava/lang/Object;Lscala/Function2;)Ljava/lang/Object;";
            asm.invokestatic(
                "scala/collection/ArrayOps",
                &format!("{}$extension", s.name),
                desc,
            );
            maybe_unbox_erased_result(asm, ctx, desc, result_ty);
            return;
        }
        if s.name == "count" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "count$extension",
                "(Ljava/lang/Object;Lscala/Function1;)I",
            );
            return;
        }
        if s.name == "forall" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "forall$extension",
                "(Ljava/lang/Object;Lscala/Function1;)Z",
            );
            return;
        }
        if s.name == "scanLeft" {
            let desc = "(Ljava/lang/Object;Ljava/lang/Object;Lscala/Function2;Lscala/reflect/ClassTag;)Ljava/lang/Object;";
            asm.invokestatic("scala/collection/ArrayOps", "scanLeft$extension", desc);
            maybe_unbox_erased_result(asm, ctx, desc, result_ty);
            return;
        }
        if s.name == "size" || s.name == "length" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "size$extension",
                "(Ljava/lang/Object;)I",
            );
            return;
        }
        if s.name == "isEmpty" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "isEmpty$extension",
                "(Ljava/lang/Object;)Z",
            );
            return;
        }
        if s.name == "nonEmpty" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "nonEmpty$extension",
                "(Ljava/lang/Object;)Z",
            );
            return;
        }
        if s.name == "find" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "find$extension",
                "(Ljava/lang/Object;Lscala/Function1;)Lscala/Option;",
            );
            return;
        }
        if s.name == "contains" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "contains$extension",
                "(Ljava/lang/Object;Ljava/lang/Object;)Z",
            );
            return;
        }
        if s.name == "takeRight" || s.name == "dropRight" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                &format!("{}$extension", s.name),
                "(Ljava/lang/Object;I)Ljava/lang/Object;",
            );
            maybe_unbox_erased_result(
                asm,
                ctx,
                "(Ljava/lang/Object;I)Ljava/lang/Object;",
                result_ty,
            );
            return;
        }
        if s.name == "takeWhile" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "takeWhile$extension",
                "(Ljava/lang/Object;Lscala/Function1;)Ljava/lang/Object;",
            );
            maybe_unbox_erased_result(
                asm,
                ctx,
                "(Ljava/lang/Object;Lscala/Function1;)Ljava/lang/Object;",
                result_ty,
            );
            return;
        }
        if s.name == "indices" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "indices$extension",
                "(Ljava/lang/Object;)Lscala/collection/immutable/Range;",
            );
            return;
        }
        if s.name == "lengthCompare" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "lengthCompare$extension",
                "(Ljava/lang/Object;I)I",
            );
            return;
        }
        if s.name == "filterNot" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "filterNot$extension",
                "(Ljava/lang/Object;Lscala/Function1;)Ljava/lang/Object;",
            );
            maybe_unbox_erased_result(
                asm,
                ctx,
                "(Ljava/lang/Object;Lscala/Function1;)Ljava/lang/Object;",
                result_ty,
            );
            return;
        }
        if s.name == "headOption" || s.name == "lastOption" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                &format!("{}$extension", s.name),
                "(Ljava/lang/Object;)Lscala/Option;",
            );
            return;
        }
        if s.name == "partition" || s.name == "span" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                &format!("{}$extension", s.name),
                "(Ljava/lang/Object;Lscala/Function1;)Lscala/Tuple2;",
            );
            return;
        }
        if s.name == "splitAt" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "splitAt$extension",
                "(Ljava/lang/Object;I)Lscala/Tuple2;",
            );
            return;
        }
        if s.name == "zipWithIndex" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "zipWithIndex$extension",
                "(Ljava/lang/Object;)[Lscala/Tuple2;",
            );
            return;
        }
        if s.name == "knownSize" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "knownSize$extension",
                "(Ljava/lang/Object;)I",
            );
            return;
        }
        if s.name == "sizeCompare" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "sizeCompare$extension",
                "(Ljava/lang/Object;I)I",
            );
            return;
        }
        if s.name == "lengthIs" || s.name == "sizeIs" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                &format!("{}$extension", s.name),
                "(Ljava/lang/Object;)I",
            );
            return;
        }
        if s.name == "indexOf" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "indexOf$extension",
                "(Ljava/lang/Object;Ljava/lang/Object;I)I",
            );
            return;
        }
        if s.name == "copyToArray" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "copyToArray$extension",
                "(Ljava/lang/Object;Ljava/lang/Object;)I",
            );
            return;
        }
        if s.name == "iterator" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "iterator$extension",
                "(Ljava/lang/Object;)Lscala/collection/Iterator;",
            );
            return;
        }
        if s.name == "toSeq" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "toSeq$extension",
                "(Ljava/lang/Object;)Lscala/collection/immutable/Seq;",
            );
            return;
        }
        if s.name == "toIndexedSeq" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "toIndexedSeq$extension",
                "(Ljava/lang/Object;)Lscala/collection/immutable/IndexedSeq;",
            );
            return;
        }
        if s.name == "groupBy" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "groupBy$extension",
                "(Ljava/lang/Object;Lscala/Function1;)Lscala/collection/immutable/Map;",
            );
            return;
        }
        if s.name == "sortBy" {
            let desc =
                "(Ljava/lang/Object;Lscala/Function1;Lscala/math/Ordering;)Ljava/lang/Object;";
            asm.invokestatic("scala/collection/ArrayOps", "sortBy$extension", desc);
            maybe_unbox_erased_result(asm, ctx, desc, result_ty);
            return;
        }
        if s.name == "sorted" {
            let desc = "(Ljava/lang/Object;Lscala/math/Ordering;)Ljava/lang/Object;";
            asm.invokestatic("scala/collection/ArrayOps", "sorted$extension", desc);
            maybe_unbox_erased_result(asm, ctx, desc, result_ty);
            return;
        }
        if s.name == "sortWith" {
            let desc = "(Ljava/lang/Object;Lscala/Function2;)Ljava/lang/Object;";
            asm.invokestatic("scala/collection/ArrayOps", "sortWith$extension", desc);
            maybe_unbox_erased_result(asm, ctx, desc, result_ty);
            return;
        }
        if s.name == "zipAll" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "zipAll$extension",
                "(Ljava/lang/Object;Lscala/collection/Iterable;Ljava/lang/Object;Ljava/lang/Object;)[Lscala/Tuple2;",
            );
            return;
        }
        if s.name == "indexWhere" {
            if param_count(ctx.st, id) < 2 {
                // 1-arg overload: `from` defaults to 0.
                asm.iconst(0);
            }
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "indexWhere$extension",
                "(Ljava/lang/Object;Lscala/Function1;I)I",
            );
            return;
        }
        if s.name == "lastIndexOf" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "lastIndexOf$extension",
                "(Ljava/lang/Object;Ljava/lang/Object;I)I",
            );
            return;
        }
        if s.name == "patch" {
            let desc = "(Ljava/lang/Object;ILscala/collection/IterableOnce;ILscala/reflect/ClassTag;)Ljava/lang/Object;";
            asm.invokestatic("scala/collection/ArrayOps", "patch$extension", desc);
            maybe_unbox_erased_result(asm, ctx, desc, result_ty);
            return;
        }
        if s.name == "updated" {
            let desc =
                "(Ljava/lang/Object;ILjava/lang/Object;Lscala/reflect/ClassTag;)Ljava/lang/Object;";
            asm.invokestatic("scala/collection/ArrayOps", "updated$extension", desc);
            maybe_unbox_erased_result(asm, ctx, desc, result_ty);
            return;
        }
        if s.name == "appended" || s.name == "prepended" {
            let desc =
                "(Ljava/lang/Object;Ljava/lang/Object;Lscala/reflect/ClassTag;)Ljava/lang/Object;";
            asm.invokestatic(
                "scala/collection/ArrayOps",
                &format!("{}$extension", s.name),
                desc,
            );
            maybe_unbox_erased_result(asm, ctx, desc, result_ty);
            return;
        }
        if s.name == "concat" || s.name == "++" {
            let ext_name = if s.name == "++" {
                "$plus$plus$extension"
            } else {
                "concat$extension"
            };
            let desc = "(Ljava/lang/Object;Lscala/collection/IterableOnce;Lscala/reflect/ClassTag;)Ljava/lang/Object;";
            asm.invokestatic("scala/collection/ArrayOps", ext_name, desc);
            maybe_unbox_erased_result(asm, ctx, desc, result_ty);
            return;
        }
        if s.name == "toList" {
            asm.invokeinterface(
                "scala/collection/IterableOnceOps",
                "toList",
                "()Lscala/collection/immutable/List;",
            );
            return;
        }
        if s.name == "toSet" {
            asm.invokeinterface(
                "scala/collection/IterableOnceOps",
                "toSet",
                "()Lscala/collection/immutable/Set;",
            );
            return;
        }
        if s.name == "toVector" {
            asm.invokeinterface(
                "scala/collection/IterableOnceOps",
                "toVector",
                "()Lscala/collection/immutable/Vector;",
            );
            return;
        }
        if s.name == "toBuffer" {
            asm.invokeinterface(
                "scala/collection/IterableOnceOps",
                "toBuffer",
                "()Lscala/collection/mutable/Buffer;",
            );
            return;
        }
        if s.name == "mkString" {
            let n = param_count(ctx.st, id);
            let desc = match n {
                0 => "()Ljava/lang/String;",
                1 => "(Ljava/lang/String;)Ljava/lang/String;",
                _ => "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
            };
            asm.invokeinterface("scala/collection/IterableOnceOps", "mkString", desc);
            return;
        }
        if s.name == "reduce" || s.name == "reduceLeft" {
            asm.invokeinterface(
                "scala/collection/IterableOnceOps",
                &s.name,
                "(Lscala/Function2;)Ljava/lang/Object;",
            );
            maybe_unbox_erased_result(asm, ctx, "(Lscala/Function2;)Ljava/lang/Object;", result_ty);
            return;
        }
        if s.name == "sum" || s.name == "product" {
            asm.invokeinterface(
                "scala/collection/IterableOnceOps",
                &s.name,
                "(Lscala/math/Numeric;)Ljava/lang/Object;",
            );
            maybe_unbox_erased_result(
                asm,
                ctx,
                "(Lscala/math/Numeric;)Ljava/lang/Object;",
                result_ty,
            );
            return;
        }
        if s.name == "min" || s.name == "max" {
            asm.invokeinterface(
                "scala/collection/IterableOnceOps",
                &s.name,
                "(Lscala/math/Ordering;)Ljava/lang/Object;",
            );
            maybe_unbox_erased_result(
                asm,
                ctx,
                "(Lscala/math/Ordering;)Ljava/lang/Object;",
                result_ty,
            );
            return;
        }
        if s.name == "minBy" || s.name == "maxBy" {
            asm.invokeinterface(
                "scala/collection/IterableOnceOps",
                &s.name,
                "(Lscala/Function1;Lscala/math/Ordering;)Ljava/lang/Object;",
            );
            maybe_unbox_erased_result(
                asm,
                ctx,
                "(Lscala/Function1;Lscala/math/Ordering;)Ljava/lang/Object;",
                result_ty,
            );
            return;
        }
        // Everything else on `ArrayOps` goes out as its `$extension` static.
        // The descriptor used to be hard-coded as `(Object)Object`, which is
        // right only for a member that takes nothing but the receiver
        // (`head`, `reverse`, `isEmpty`, …). For one that takes arguments the
        // call named a signature the arguments did not fit: `a :+ x` pushed
        // the array, the element and the `ClassTag` and then invoked
        // `$colon$plus$extension(Object)Object`, which the verifier rejects
        // ("Inconsistent stackmap frames" at the first branch that joins).
        // The pickled signature is the erasure nsc emits, so read it off the
        // symbol; only the receiver has to be written by hand, because
        // `ArrayOps`' underlying `Array[A]` erases to `Object` and not to
        // `[Ljava/lang/Object;`.
        let inst = method_desc_from_sym(ctx.st, id);
        let desc = format!(
            "(Ljava/lang/Object;{}",
            inst.strip_prefix('(').unwrap_or(&inst)
        );
        asm.invokestatic(
            "scala/collection/ArrayOps",
            &format!("{}$extension", s.name),
            &desc,
        );
        maybe_unbox_erased_result(asm, ctx, &desc, result_ty);
        return;
    }
    let desc = value_extension_desc(ctx.st, id);
    if let Some(ext_owner) = value_extension_module(ctx.st, id) {
        if !module_pushed {
            // Paren-less selection: no arguments follow, so the module can be
            // pushed on top of the receiver and swapped under it.
            let n_args = count_value_ext_args(&desc);
            asm.getstatic(&ext_owner, "MODULE$", &format!("L{ext_owner};"));
            if n_args == 0 {
                asm.swap();
            } else {
                asm.dup_x2();
                asm.pop();
            }
        }
        asm.invokevirtual(&ext_owner, &format!("{}$extension", s.name), &desc);
        maybe_unbox_erased_result(asm, ctx, &desc, result_ty);
        return;
    }
    asm.invokestatic(&owner, &format!("{}$extension", s.name), &desc);
    maybe_unbox_erased_result(asm, ctx, &desc, result_ty);
}

/// The companion module a value class's `$extension` methods live on, when the
/// call has to go through it.
///
/// nsc always declares them there, but it *also* emits static forwarders on
/// the value class itself -- for a **top-level** one. A nested value class
/// (`scala.Predef.ArrowAssoc`, `fs2.Stream.PartiallyAppliedFromIterator`, and
/// every one of slick's) gets no forwarders, so the module is the only way in.
/// A value class this run compiles is different again: `emit_value_extension`
/// puts the statics on the class itself, whatever its nesting.
pub(crate) fn value_extension_module(st: &SymbolTable, id: SymbolId) -> Option<String> {
    let owner_id = st.get(id).owner;
    if st.source_value_classes.contains(&owner_id) {
        return None;
    }
    let owner = class_internal(st, owner_id);
    owner.contains('$').then(|| format!("{owner}$"))
}

pub(crate) fn desc_ret_sort(desc: &str) -> JvmSort {
    match desc
        .rsplit_once(')')
        .map(|(_, r)| r)
        .unwrap_or("V")
        .as_bytes()
    {
        [b'V'] => JvmSort::Void,
        [b'J'] => JvmSort::Long,
        [b'D'] => JvmSort::Double,
        [b'F'] => JvmSort::Float,
        [b'L', ..] | [b'[', ..] => JvmSort::Ref,
        _ => JvmSort::Int,
    }
}

pub(crate) fn count_value_ext_args(desc: &str) -> usize {
    // Descriptor includes the extension receiver as the first argument.
    let inner = desc
        .split_once(')')
        .map(|(a, _)| a.trim_start_matches('('))
        .unwrap_or("");
    let mut n: usize = 0;
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        n += 1;
        match c {
            'L' => while chars.next().is_some_and(|ch| ch != ';') {},
            '[' => {
                while chars.peek() == Some(&'[') {
                    chars.next();
                }
                if chars.peek() == Some(&'L') {
                    chars.next();
                    while chars.next().is_some_and(|ch| ch != ';') {}
                } else {
                    chars.next();
                }
            }
            _ => {}
        }
    }
    n.saturating_sub(1)
}

/// Push the receiver of a call from inside `class C(val u: U) extends AnyVal`
/// to another of `C`'s own methods, which goes out as `m$extension(u, …)`.
/// Answers whether it pushed one; the caller falls back to `gen_receiver`.
///
/// `this` inside a value class denotes the *box*, and the `$extension` static
/// wants the underlying value. nsc emits `aload_0; invokevirtual u()` in the
/// instance method and the bare underlying slot in the `$extension` static.
/// Without this, `def a(n: Int) = b(n)` handed `b$extension(U, int)` a `C`:
/// the instance method passed `this` straight through, and the `$extension`
/// re-boxed slot 0 with a `new C(u)` it had just been handed unwrapped. The
/// first shape is a `VerifyError` when the parameter is a class; the second
/// only fails at run time whenever `U` is an interface, since JVMS 4.10.1.2
/// makes every reference assignable to one.
pub(crate) fn gen_value_self_receiver(asm: &mut Assembler, ctx: &EmitCtx, fun: &Tree) -> bool {
    if fun.sym.is_none() {
        return false;
    }
    let owner = ctx.st.get(fun.sym).owner;
    if owner != ctx.class_sym || !ctx.st.is_value_class(owner) {
        return false;
    }
    if ctx.st.get(fun.sym).flags.contains(Flags::STATIC) {
        return false;
    }
    if !receiver_is_bare_this(fun) {
        return false;
    }
    let Some(&f) = ctx.st.get(owner).ctor_fields.first() else {
        return false;
    };
    // In an `$extension` static the underlying value *is* slot 0. A lambda
    // lifted out of one holds the box instead (`load_this` built it when the
    // capture was made), which is what `ctx.outer*` distinguishes.
    if ctx.outer.is_none() && ctx.outer_slot.is_none() {
        if let Some((_, _, sort)) = &ctx.value_ext {
            load(asm, 0, *sort);
            return true;
        }
    }
    let under = ctx.st.get(f).ty.clone();
    load_this(asm, ctx);
    asm.invokevirtual(
        &class_internal(ctx.st, owner),
        &ctx.st.get(f).name,
        &format!("(){}", jvm_desc(ctx.st, &under)),
    );
    true
}

/// Whether the callee's receiver is the enclosing template's own `this`,
/// written or implied. `TypeApply` / `Typed` wrappers are peeled the same way
/// `gen_receiver` peels them.
pub(crate) fn receiver_is_bare_this(fun: &Tree) -> bool {
    match &fun.kind {
        TreeKind::TypeApply { fun: inner, .. } | TreeKind::Typed { expr: inner, .. } => {
            receiver_is_bare_this(inner)
        }
        TreeKind::Select { qual, .. } => {
            matches!(&qual.kind, TreeKind::This { qual: None })
        }
        TreeKind::Ident { .. } => true,
        _ => false,
    }
}

pub(crate) fn box_value_class_receiver(
    asm: &mut Assembler,
    ctx: &EmitCtx,
    owner: SymbolId,
    qual: &Tree,
) {
    let under = ctx.st.value_class_underlying(owner).unwrap_or(Type::Any);
    if is_jvm_primitive(&under) {
        return;
    }
    let src = peel_identity_arg(ctx, qual);
    if is_jvm_primitive(&src.ty) {
        emit_box(asm, &src.ty);
    }
}

pub(crate) fn peel_identity_arg<'a>(ctx: &EmitCtx, tree: &'a Tree) -> &'a Tree {
    if let TreeKind::Apply { fun, args } = &tree.kind {
        let ic = if !fun.sym.is_none() {
            ctx.st.get(fun.sym).intrinsic
        } else {
            Intrinsic::None
        };
        if matches!(
            ic,
            Intrinsic::Identity | Intrinsic::Any2StringAdd | Intrinsic::WrapArrowAssoc
        ) {
            if let Some(a) = args.first() {
                return a;
            }
        }
    }
    tree
}

pub(crate) fn value_extension_desc(st: &SymbolTable, id: SymbolId) -> String {
    let owner = st.get(id).owner;
    let under = st.value_class_underlying(owner).unwrap_or(Type::Int);
    let inst = method_desc_from_sym(st, id);
    let rest = inst.strip_prefix('(').unwrap_or(&inst);
    format!("({}{}", jvm_desc(st, &under), rest)
}

/// Push the receiver for a member of `mcls`, given the qualifier as written.
///
/// `o.P.q` names the object itself, so the qualifier already evaluates to it.
/// `o.K(2)` on a nested case class instead names the *enclosing* instance and
/// leaves the companion implicit, so the accessor still has to run.
pub(crate) fn gen_module_member_receiver(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    qual: &Tree,
    mcls: SymbolId,
) {
    let Some(outer) = member_module_outer(ctx.st, mcls) else {
        let jvm = class_internal(ctx.st, mcls);
        asm.getstatic(&jvm, "MODULE$", &format!("L{jvm};"));
        return;
    };
    // A qualifier that is itself a paren-less accessor (`universe.Liftable`,
    // whose symbol is `def Liftable: Liftables$Liftable$`) still carries its
    // *method* type here, which `class_sym_of` has no answer for. Unwidened,
    // the qualifier was thrown away and the object reached through
    // `load_module_instance` -- `aload_0`, the enclosing *source* class, and
    // a `ClassCastException` at the first call.
    let qty = match &qual.ty {
        Type::Method { paramss, ret } if paramss.iter().all(|c| c.is_empty()) => (**ret).clone(),
        other => other.clone(),
    };
    let qcls = ctx.st.class_sym_of(&qty);
    if qcls == Some(mcls) {
        gen_expr(asm, frame, ctx, qual);
        return;
    }
    // The qualifier is a *value* of some class: it is the enclosing instance,
    // whether or not the symbol table can see the inheritance. A library
    // class's pickled parents are attached one level at a time, so
    // `is_owner_compatible(JavaUniverse, Liftables)` is routinely false for a
    // chain that really does hold -- and falling through to
    // `load_module_instance` then reached for the *enclosing source class's*
    // instance instead (`aload_0`), which is a `ClassCastException` at the
    // first call. nsc emits exactly the cast below.
    if let Some(q) = qcls {
        gen_expr(asm, frame, ctx, qual);
        if !is_owner_compatible(ctx.st, q, outer) {
            let jn = class_internal(ctx.st, outer);
            if !jn.is_empty() && jn != "java/lang/Object" && !jn.starts_with('(') {
                asm.checkcast(&jn);
            }
        }
        invoke_module_accessor(asm, ctx.st, outer, mcls);
        return;
    }
    load_module_instance(asm, ctx, mcls);
}

/// Push the receiver of `owner`'s member from the qualifier as written.
pub(crate) fn gen_select_receiver(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    qual: &Tree,
    owner: SymbolId,
) {
    if is_module_class(ctx.st, owner) {
        let mcls = module_class_id(ctx.st, owner);
        if member_module_outer(ctx.st, mcls).is_some() {
            gen_module_member_receiver(asm, frame, ctx, qual, mcls);
            return;
        }
    }
    gen_expr(asm, frame, ctx, qual);
    // A member selected *on* a `Unit` value needs a real receiver, exactly as
    // in `gen_receiver`: `().toString`, `().hashCode`, `().isInstanceOf[T]`
    // all invoke on the `BoxedUnit` singleton, and the qualifier expression
    // left nothing on the stack.
    adapt_unit_arg(asm, ctx, qual, &qual.ty);
}

pub(crate) fn gen_receiver(asm: &mut Assembler, frame: &mut Frame, ctx: &EmitCtx, fun: &Tree) {
    if !fun.sym.is_none() && ctx.st.get(fun.sym).flags.contains(Flags::STATIC) {
        return;
    }
    match &fun.kind {
        // `o.P.apply[T](x)`: an explicit type application wraps the `Select`,
        // and the fallback arm below reads only `fun.sym` -- for a member
        // `object` it loaded `this` and the enclosing instance's accessor
        // (`aload_0; checkcast Liftables; invokeinterface Liftables.Liftable()`
        // for `universe.Liftable[String](f)`), which is a
        // `ClassCastException` at the first call from a compile that reported
        // nothing. Peel it and take the `Select` path, which knows how to
        // reach the object through its qualifier.
        TreeKind::TypeApply { fun: inner, .. } | TreeKind::Typed { expr: inner, .. }
            if matches!(inner.kind, TreeKind::Select { .. }) =>
        {
            gen_receiver(asm, frame, ctx, inner)
        }
        TreeKind::Select { qual, .. } => {
            if !fun.sym.is_none() {
                let s = ctx.st.get(fun.sym);
                if matches!(s.kind, SymKind::Module | SymKind::ModuleClass) {
                    let mcls = module_class_id(ctx.st, fun.sym);
                    // A member `object` comes from its enclosing instance's
                    // accessor; `gen_select` knows how to reach it.
                    if member_module_outer(ctx.st, mcls).is_some() {
                        gen_expr(asm, frame, ctx, fun);
                    } else {
                        let jvm = class_internal(ctx.st, mcls);
                        asm.getstatic(&jvm, "MODULE$", &format!("L{jvm};"));
                    }
                    return;
                }
                if s.kind == SymKind::Method && is_module_class(ctx.st, s.owner) {
                    let mcls = module_class_id(ctx.st, s.owner);
                    gen_module_member_receiver(asm, frame, ctx, qual, mcls);
                    return;
                }
                if s.kind == SymKind::Package {
                    return;
                }
            }
            gen_expr(asm, frame, ctx, qual);
            // A method invoked *on* a `Unit` value gets a real receiver: the
            // receiver is a value position like any other, and `Unit` erases
            // to `scala/runtime/BoxedUnit` there. The expression itself left
            // nothing on the stack (`()`, `g()`, a `Unit` local), so the
            // singleton is materialised here -- otherwise `() == ()` popped an
            // operand that was never pushed (`VerifyError: Operand stack
            // underflow`).
            adapt_unit_arg(asm, ctx, qual, &qual.ty);
            // `x.toString` on an `Int` calls `java/lang/Integer.toString`, so
            // the primitive receiver has to be boxed first.
            if is_jvm_primitive(&qual.ty) && !is_unit_like(&qual.ty) && !fun.sym.is_none() {
                let s = ctx.st.get(fun.sym);
                let owner_jvm = class_internal(ctx.st, s.owner);
                if matches!(s.intrinsic, Intrinsic::None)
                    && !ctx.st.is_value_class(s.owner)
                    && (is_boxed_primitive(&owner_jvm) || owner_jvm == "java/lang/Object")
                {
                    emit_box(asm, &qual.ty);
                }
            }
        }
        _ => {
            if fun.sym.is_none() {
                load_this(asm, ctx);
                return;
            }
            let s = ctx.st.get(fun.sym);
            if matches!(s.kind, SymKind::Module | SymKind::ModuleClass) {
                load_module_instance(asm, ctx, module_class_id(ctx.st, fun.sym));
                return;
            }
            let owner = s.owner;
            if is_module_class(ctx.st, owner) {
                load_module_instance(asm, ctx, module_class_id(ctx.st, owner));
            } else if owner == ctx.class_sym || owner.is_none() {
                load_this(asm, ctx);
            } else if !is_owner_compatible(ctx.st, ctx.class_sym, owner)
                && outer_chain_reaches(ctx.st, ctx.class_sym, owner)
            {
                // The method lives further out than `this`: a class nested in
                // another class reaches the enclosing instance's methods
                // through `$outer`, exactly as reading an enclosing *field*
                // already did. Casting `this` to the enclosing class instead
                // compiled clean and threw `ClassCastException` on the first
                // call (`class Outer { def deco(s: String) = …; class Inner {
                // def q(c: String) = deco(c) } }`).
                load_owner_instance(asm, ctx, owner);
            } else {
                load_this(asm, ctx);
                maybe_checkcast_owner(asm, ctx, owner);
            }
        }
    }
}

/// A member completed from a pickle that carries an implicit clause of its own
/// (`SortedSetOps.map[B](f)(implicit ord: Ordering[B])`).
///
/// The hardcoded stdlib table below spells the `IterableOps` shape of these
/// names -- `map:(Lscala/Function1;)Ljava/lang/Object;` -- and the witness is
/// already on the stack by the time it runs, so `TreeSet.map(f)` went out as a
/// one-argument call with two arguments pushed (`IncompatibleClassChangeError`
/// at the first invocation). Those calls take the general path, which uses the
/// erased descriptor the pickle recorded. The prelude's own sorted factories
/// (`TreeSet(1, 2)`, whose `apply` takes an `Ordering` too) are hand-written,
/// not pickled, and keep their table entry.
pub(crate) fn pickled_with_implicit_clause(st: &SymbolTable, id: SymbolId) -> bool {
    let s = st.get(id);
    !s.pickled_origin.is_empty()
        && s.paramss
            .iter()
            .any(|c| c.iter().any(|p| st.get(*p).flags.contains(Flags::IMPLICIT)))
}
