//! Argument passing and the coercions around it: Java and Scala varargs,
//! `Unit` adaptation, boxing and unboxing, `asInstanceOf` / `isInstanceOf`,
//! array creation and access, `FunctionN.apply`, and the `Predef` intrinsics
//! (`println`, `assert` / `require`, `ArrowAssoc`).

use crate::code::Assembler;
use crate::gen::*;
use scala_rs_parser::{Flags, SymbolId, Tree, TreeKind, Type};
use scala_rs_typer::{Intrinsic, SymKind, SymbolTable};

pub(crate) fn gen_java_varargs_array(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    args: &[Tree],
    elem: &Type,
) {
    let n = args.len() as i32;
    asm.iconst(n);
    match elem {
        Type::String => asm.anewarray("java/lang/String"),
        Type::Class { sym, .. } | Type::ModuleRef(sym) => {
            asm.anewarray(&class_internal(ctx.st, *sym));
        }
        _ => asm.anewarray("java/lang/Object"),
    }
    for (i, a) in args.iter().enumerate() {
        asm.dup();
        asm.iconst(i as i32);
        gen_expr(asm, frame, ctx, a);
        if is_jvm_primitive(&a.ty) {
            emit_box(asm, &a.ty);
        }
        asm.aastore();
    }
}

pub(crate) fn gen_wrap_varargs(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    args: &[Tree],
    elem: &Type,
) {
    let n = args.len() as i32;
    // An *empty* varargs argument is `Nil`, not an empty `ArraySeq`: nsc emits
    // `scala/collection/immutable/Nil$.MODULE$` and the difference is visible,
    // because the callee may print what it was handed (`run/t5966` expects
    // `List()` where we printed `ArraySeq()`).
    if args.is_empty() {
        asm.getstatic(
            "scala/collection/immutable/Nil$",
            "MODULE$",
            "Lscala/collection/immutable/Nil$;",
        );
        return;
    }
    let all_int = !args.is_empty()
        && args.iter().all(|a| matches!(a.ty, Type::Int))
        && matches!(
            elem,
            Type::Int | Type::Any | Type::AnyRef | Type::TypeParam(_)
        );
    let all_unit = !args.is_empty() && args.iter().all(is_unit_varargs_elem);
    asm.getstatic(
        "scala/runtime/ScalaRunTime$",
        "MODULE$",
        "Lscala/runtime/ScalaRunTime$;",
    );
    asm.iconst(n);
    if all_int {
        asm.newarray(10); // T_INT
        for (i, a) in args.iter().enumerate() {
            asm.dup();
            asm.iconst(i as i32);
            gen_expr(asm, frame, ctx, a);
            asm.iastore();
        }
        asm.invokevirtual(
            "scala/runtime/ScalaRunTime$",
            "wrapIntArray",
            "([I)Lscala/collection/immutable/ArraySeq;",
        );
    } else if all_unit && ctx.library_abi {
        // nsc `Array((), ())` uses wrapUnitArray of BoxedUnit.UNIT, not null.
        // Erasure may wrap some elems in `$box`, so treat those as Unit too.
        asm.anewarray("scala/runtime/BoxedUnit");
        for (i, a) in args.iter().enumerate() {
            asm.dup();
            asm.iconst(i as i32);
            gen_varargs_elem(asm, frame, ctx, a);
            asm.aastore();
        }
        asm.invokevirtual(
            "scala/runtime/ScalaRunTime$",
            "wrapUnitArray",
            "([Lscala/runtime/BoxedUnit;)Lscala/collection/immutable/ArraySeq;",
        );
    } else {
        asm.anewarray("java/lang/Object");
        for (i, a) in args.iter().enumerate() {
            asm.dup();
            asm.iconst(i as i32);
            gen_varargs_elem(asm, frame, ctx, a);
            asm.aastore();
        }
        asm.invokevirtual(
            "scala/runtime/ScalaRunTime$",
            "wrapRefArray",
            "([Ljava/lang/Object;)Lscala/collection/immutable/ArraySeq;",
        );
    }
}

/// nsc's erasure adaptation, for an argument whose static type the typer only
/// knows as `Any`.
///
/// `scala.reflect`'s API is written in abstract type members with compound
/// bounds -- `type TermName >: Null <: TermNameApi with Name`, `type Name >:
/// Null <: NameApi` -- which the typer carries as `Any` and the *classfile*
/// erases to `Names$TermNameApi` and `Names$NameApi`. The JVM knows of no
/// relation between those two classes, so `Ident(TermName("tag"))` does not
/// verify: "Type 'scala/reflect/api/Names$TermNameApi' is not assignable to
/// 'scala/reflect/api/Names$NameApi'". nsc emits a `checkcast` to the
/// parameter's erasure at exactly this point; so does this.
///
/// Scoped to an argument the typer typed as `Any` (or an abstract type
/// member) whose parameter descriptor names a *specific* class: an ordinary
/// call cannot pass an `Any` where a class is wanted, so every other call
/// emits what it emitted before.
pub(crate) fn adapt_type_member_arg(
    asm: &mut Assembler,
    ctx: &EmitCtx,
    aty: &Type,
    pty: &Type,
    declared: Option<&str>,
) {
    let opaque = matches!(
        aty,
        Type::Any | Type::AnyRef | Type::TypeMember(_) | Type::NoType
    ) || matches!(pty, Type::TypeMember(_));
    if is_jvm_primitive(pty) {
        return;
    }
    let pd = declared
        .map(str::to_string)
        .unwrap_or_else(|| jvm_desc(ctx.st, pty));
    if pd == "Ljava/lang/Object;" {
        return;
    }
    let Some(cls) = pd.strip_prefix('L').and_then(|d| d.strip_suffix(';')) else {
        return;
    };
    // Only when the value on the stack really is some *other* class: a value
    // already of that class, or one whose class the assembler does not track,
    // is left alone.
    let Some(top) = asm.top_object() else { return };
    if top == cls {
        return;
    }
    if !opaque {
        // The argument's own erasure is a class, and it may still be one the
        // verifier will not accept here. An abstract type member erases
        // through nsc's `intersectionDominator`, which drops the other parts
        // of a compound bound: scala-reflect's `type TermName >: Null <:
        // TermNameApi with Name` is `Names$TermNameApi`, an *interface* that
        // does not extend the abstract class `Names$NameApi` -- only `Name`
        // brings that in. Passing a `TermName` where the quasiquote support
        // API takes a `NameApi` needs the cast nsc emits here, or the method
        // fails to verify ("Type Names$TermNameApi is not assignable to
        // Names$NameApi").
        //
        // Only a *class* target is cast: JVMS 4.10.1.2 makes every class type
        // assignable to an interface type, so an interface parameter never
        // owes one, and casting there would be noise on every call.
        if is_interface_jvm(ctx.st, cls) || jvm_assignable(ctx.st, top, cls) {
            return;
        }
    }
    asm.checkcast(cls);
}

/// Whether the JVM class named `jvm` is an interface, as far as the symbol
/// table knows. Unknown names answer `true`: an interface target never owes a
/// cast, so an unknown one is left alone rather than cast blindly.
pub(crate) fn is_interface_jvm(st: &SymbolTable, jvm: &str) -> bool {
    match st.find_class_by_jvm(jvm) {
        Some(c) => is_interface_sym(st, c),
        None => true,
    }
}

/// Whether a value the assembler tracks as JVM class `from` reaches a
/// parameter of class `to` without a `checkcast`. Answers `false` when either
/// class is unknown here: a redundant cast to a class the value really has is
/// a no-op, whereas a missing one is a `VerifyError`.
pub(crate) fn jvm_assignable(st: &SymbolTable, from: &str, to: &str) -> bool {
    if from == to || to == "java/lang/Object" {
        return true;
    }
    let (Some(f), Some(t)) = (st.find_class_by_jvm(from), st.find_class_by_jvm(to)) else {
        return false;
    };
    f == t
        || st
            .base_type_seq(&Type::Class {
                sym: f,
                args: vec![],
            })
            .iter()
            .any(|b| st.class_sym_of(b) == Some(t))
}

/// Materialise the argument a `Unit` parameter expects. `Unit` erases to
/// `scala/runtime/BoxedUnit` in parameter position, so the call really does
/// push one -- but the expression that produced it left nothing on the stack
/// (`f(())`, `f(g())` alike). Erasure sometimes hands over an expression that
/// already produced a reference (`$box`, a generic `T` result); that one *is*
/// the box, and only needs narrowing from `Object`.
/// `adapt_unit_arg` for the receiver of `asInstanceOf` / `isInstanceOf`.
///
/// The *parameter* case can trust `NoType` to mean `Unit` -- a parameter always
/// has a written or inferred type, and `jvm_desc_val` erases both the same way.
/// A *qualifier* cannot: `NoType` there means the typer recorded nothing, and
/// `gen_expr` has already left whatever the expression really produced on the
/// stack. Materialising a `BoxedUnit` on top of it is one value too many.
/// slick's `ScalaBaseType.scalaOrderingFor` returns a lambda whose parameters
/// come out `<notype>` (the SAM's element type is not borrowed from the
/// overridden `def scalaOrderingFor(ord: Ordering): Ordering[T]`), so
/// `x.asInstanceOf[AnyRef] eq null` compared `BoxedUnit.UNIT` against `null`
/// and left `x` stranded: `VerifyError: Inconsistent stackmap frames`.
pub(crate) fn adapt_unit_qualifier(asm: &mut Assembler, ctx: &EmitCtx, a: &Tree) {
    if !matches!(a.ty.widen_constant(), Type::Unit) {
        return;
    }
    adapt_unit_arg(asm, ctx, a, &Type::Unit);
}

pub(crate) fn adapt_unit_arg(asm: &mut Assembler, ctx: &EmitCtx, a: &Tree, pty: &Type) {
    if !erases_to_boxed_unit(pty) {
        return;
    }
    if unit_leaves_boxed_ref(a, ctx.st) {
        asm.checkcast(BOXED_UNIT);
    } else {
        emit_boxed_unit(asm);
    }
}

pub(crate) fn gen_call_args(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    args: &[Tree],
    param_tys: &[Type],
    box_prims: bool,
    java_varargs: bool,
    method: SymbolId,
) {
    let param_ids: Vec<SymbolId> = if method.is_none() {
        Vec::new()
    } else {
        ctx.st.get(method).params.clone()
    };
    // The parameter descriptors the call really names. `jvm_desc` of the
    // parameter's *type* is not the same thing for an abstract type member --
    // it comes out `Object` -- and it is the descriptor in the `Methodref`
    // that the verifier checks the argument against.
    let declared: Vec<String> = if method.is_none() {
        Vec::new()
    } else {
        crate::code::param_descs(&method_desc_from_sym(ctx.st, method))
    };
    let load_arg = |asm: &mut Assembler, frame: &mut Frame, i: usize, a: &Tree| {
        let callee_synthetic =
            !method.is_none() && ctx.st.get(method).flags.contains(Flags::SYNTHETIC);
        if callee_synthetic && param_ids.get(i).is_some_and(|p| ctx.boxed_vars.contains(p)) {
            let id = match &a.kind {
                TreeKind::Ident { .. } => a.sym,
                TreeKind::Typed { expr, .. } => expr.sym,
                _ => SymbolId::NONE,
            };
            if let Some((slot, _)) = frame.get(id) {
                load(asm, slot, JvmSort::Ref);
                return;
            }
        }
        gen_expr(asm, frame, ctx, a);
        let pty = param_tys.get(i).unwrap_or(&a.ty);
        // A `Unit` parameter is a `scala/runtime/BoxedUnit` on the JVM, so an
        // argument really is pushed: `f(())` and `f(g())` alike leave nothing
        // behind and need the singleton here. Erasure sometimes hands us an
        // expression that already produced a reference (`$box`, a generic `T`
        // result) -- that one is the value, so do not push a second.
        adapt_unit_arg(asm, ctx, a, pty);
        adapt_type_member_arg(asm, ctx, &a.ty, pty, declared.get(i).map(String::as_str));
        if box_prims {
            if is_jvm_primitive(&a.ty) && !is_unit_like(&a.ty) && !is_jvm_primitive(pty) {
                emit_box(asm, &a.ty);
            } else if matches!(pty, Type::Array(_)) {
                asm.checkcast(&jvm_desc(ctx.st, pty));
            }
        }
    };
    let rep_idx = param_tys
        .iter()
        .position(|p| matches!(p, Type::Repeated(_)));
    let Some(ri) = rep_idx else {
        for (i, a) in args.iter().enumerate() {
            load_arg(asm, frame, i, a);
        }
        return;
    };
    let n_after = param_tys.len() - ri - 1;
    let n_var = args.len().saturating_sub(ri + n_after);
    for a in &args[..ri.min(args.len())] {
        gen_expr(asm, frame, ctx, a);
    }
    let var_end = (ri + n_var).min(args.len());
    let var_args = if ri <= var_end {
        &args[ri..var_end]
    } else {
        &[]
    };
    let elem = match &param_tys[ri] {
        Type::Repeated(t) => t.as_ref(),
        _ => &Type::Any,
    };
    // `f(xs: _*)` already has the sequence; nothing to wrap.
    let spliced = var_args.len() == 1 && matches!(var_args[0].ty, Type::Repeated(_));
    if spliced {
        let inner = match &var_args[0].kind {
            TreeKind::Typed { expr, .. } => expr.as_ref(),
            _ => &var_args[0],
        };
        gen_expr(asm, frame, ctx, inner);
        // ... unless it is an `Array`, which is not a `Seq` on the JVM.
        // A repeated parameter erases to `scala/collection/immutable/Seq`, so
        // `render(names: _*)` on an `Array[String]` pushed
        // `[Ljava/lang/String;` under that descriptor and died with a
        // `VerifyError` -- the typer had accepted the call (`Typed{_, _*}`
        // reads the element type straight off the array). nsc wraps it here;
        // `javap` on its output shows
        // `Predef.copyArrayToImmutableIndexedSeq(names)`.
        //
        // A *Java* varargs method is the exception: its parameter is the
        // array itself, and nsc passes an `Array` straight through.
        if !java_varargs && ctx.library_abi && matches!(inner.ty, Type::Array(_)) {
            emit_array_copy_to_immutable_seq(asm);
        }
    } else if java_varargs {
        gen_java_varargs_array(asm, frame, ctx, var_args, elem);
    } else {
        gen_wrap_varargs(asm, frame, ctx, var_args, elem);
    }
    for a in &args[var_end..] {
        gen_expr(asm, frame, ctx, a);
    }
}

/// Each primitive array has its own load opcode; `aaload` on a `[J` or a `[B`
/// is a `VerifyError`, and `iaload` on a `[Z` is one too (`boolean[]` uses the
/// `byte` opcodes).
pub(crate) fn emit_array_load(asm: &mut Assembler, elem: &Type) {
    match elem.widen_constant() {
        Type::Int => asm.iaload(),
        Type::Long => asm.laload(),
        Type::Float => asm.faload(),
        Type::Double => asm.daload(),
        Type::Byte | Type::Boolean => asm.baload(),
        Type::Char => asm.caload(),
        Type::Short => asm.saload(),
        _ => asm.aaload(),
    }
}

pub(crate) fn emit_array_store(asm: &mut Assembler, arr_ty: &Type) {
    let Type::Array(elem) = arr_ty else {
        asm.aastore();
        return;
    };
    match elem.widen_constant() {
        Type::Int => asm.iastore(),
        Type::Long => asm.lastore(),
        Type::Float => asm.fastore(),
        Type::Double => asm.dastore(),
        Type::Byte | Type::Boolean => asm.bastore(),
        Type::Char => asm.castore(),
        Type::Short => asm.sastore(),
        _ => asm.aastore(),
    }
}

pub(crate) fn load_predef_module(asm: &mut Assembler) {
    asm.getstatic("scala/Predef$", "MODULE$", "Lscala/Predef$;");
}

pub(crate) fn emit_boxed_unit(asm: &mut Assembler) {
    asm.getstatic(BOXED_UNIT, "UNIT", BOXED_UNIT_DESC);
}

/// Read a field whose Scala type may be `Unit`. Such a field really does hold
/// a `scala/runtime/BoxedUnit`, but a `Unit` *expression* leaves nothing on
/// the stack, so the value read is dropped again — which is exactly the body
/// nsc gives a `Unit` getter (`getfield; pop; return`). Nothing is lost:
/// `BoxedUnit.UNIT` is the only value the field can hold.
pub(crate) fn emit_getfield(asm: &mut Assembler, owner: &str, name: &str, desc: &str) {
    asm.getfield(owner, name, desc);
    if desc == BOXED_UNIT_DESC {
        asm.pop();
    }
}

/// Store into a field whose Scala type may be `Unit`, when the value comes
/// from an *expression* rather than from a slot: a `Unit` expression leaves
/// nothing behind, so the singleton is materialised here.
pub(crate) fn emit_putfield_from_expr(asm: &mut Assembler, owner: &str, name: &str, desc: &str) {
    fill_boxed_unit_slot(asm, desc);
    asm.putfield(owner, name, desc);
}

/// A `Unit` expression leaves nothing on the stack, but the value position it
/// was erased into -- an argument, a field, an array slot -- has to actually
/// hold the `BoxedUnit` singleton. Call this right after emitting the
/// expression, with the descriptor of the slot it is going into.
pub(crate) fn fill_boxed_unit_slot(asm: &mut Assembler, desc: &str) {
    if desc == BOXED_UNIT_DESC {
        emit_boxed_unit(asm);
    }
}

/// Unit literals, and `$box(unit)` inserted by erasure (whose result type is
/// Object). Used so `Array((), ())` still takes the wrapUnitArray path.
pub(crate) fn is_unit_varargs_elem(tree: &Tree) -> bool {
    if matches!(tree.ty.widen_constant(), Type::Unit | Type::NoType) {
        return true;
    }
    match &tree.kind {
        TreeKind::Typed { expr, .. } | TreeKind::Block { expr, .. } => is_unit_varargs_elem(expr),
        TreeKind::TypeApply { fun, .. } => is_unit_varargs_elem(fun),
        TreeKind::Apply { fun, args } => {
            peel_fun(fun).name() == Some("$box") && args.first().is_some_and(is_unit_varargs_elem)
        }
        _ => false,
    }
}

pub(crate) fn gen_varargs_elem(asm: &mut Assembler, frame: &mut Frame, ctx: &EmitCtx, a: &Tree) {
    if is_unit_varargs_elem(a) {
        gen_expr(asm, frame, ctx, a);
        if !unit_leaves_boxed_ref(a, ctx.st) {
            emit_boxed_unit(asm);
        }
    } else {
        gen_expr(asm, frame, ctx, a);
        if is_jvm_primitive(&a.ty) {
            emit_box(asm, &a.ty);
        }
    }
}

/// True when a Unit-typed expression already left a boxed ref (`BoxedUnit` or
/// `null`) on the stack — ArrayOps / generic `T` erased to Object / `$box`.
/// Unit literals leave nothing and need `BoxedUnit.UNIT`.
pub(crate) fn unit_leaves_boxed_ref(tree: &Tree, st: &SymbolTable) -> bool {
    match &tree.kind {
        TreeKind::Typed { expr, .. } => unit_leaves_boxed_ref(expr, st),
        TreeKind::TypeApply { fun, .. } => unit_leaves_boxed_ref(fun, st),
        TreeKind::Block { expr, .. } => unit_leaves_boxed_ref(expr, st),
        TreeKind::Apply { fun, .. } => {
            peel_fun(fun).name() == Some("$box") || method_erases_unit_to_ref(fun, st)
        }
        TreeKind::Select { .. } | TreeKind::Ident { .. } => method_erases_unit_to_ref(tree, st),
        _ => false,
    }
}

pub(crate) fn method_erases_unit_to_ref(fun: &Tree, st: &SymbolTable) -> bool {
    match &fun.kind {
        TreeKind::TypeApply { fun, .. } | TreeKind::Typed { expr: fun, .. } => {
            return method_erases_unit_to_ref(fun, st);
        }
        _ => {}
    }
    if fun.sym.is_none() {
        return false;
    }
    let s = st.get(fun.sym);
    // `x.asInstanceOf[Unit]` is emitted by `emit_as_instance_of`, which drops
    // the receiver: the cast's result is a `Unit` expression like any other and
    // leaves nothing, even though `asInstanceOf`'s declared result is a type
    // parameter.
    if matches!(s.intrinsic, Intrinsic::AsInstanceOf) {
        return false;
    }
    if st.get(s.owner).name == "ArrayOps" {
        return true;
    }
    match &s.ty {
        // The call leaves exactly what its own descriptor returns. A
        // `Unit`-typed expression whose callee is declared to return something
        // that erases to a reference -- a type parameter, `Any` -- really did
        // push one: `def id[A](a: A): A` is `(Object)Object` even at
        // `id(())`, and nothing pops it.
        Type::Method { ret, .. } | Type::Function { ret, .. } => {
            !matches!(ret.as_ref(), Type::Unit | Type::NoType | Type::Nothing)
        }
        Type::TypeParam(_) => true,
        _ => false,
    }
}

/// Whether this owner is a class or object defined in the unit being compiled.
/// `SymbolTable::source_classes` records an `object`'s *module* symbol, while
/// a method's owner is the module *class*, so both spellings have to match.
pub(crate) fn owner_defined_in_source(st: &SymbolTable, owner: SymbolId) -> bool {
    if st.source_classes.contains(&owner) {
        return true;
    }
    if st.get(owner).kind != SymKind::ModuleClass {
        return false;
    }
    st.source_classes
        .iter()
        .any(|&m| st.module_class_of(m) == owner)
}

/// A `Unit`-typed expression in *statement* position whose emitted code left a
/// reference behind, so it has to be dropped -- `def id[A](a: A): A` is
/// `(Object)Object` even at `id(())`, and nsc pops it too.
///
/// Deliberately narrower than `unit_leaves_boxed_ref`, which answers the same
/// question for a *value* position: only a method **defined in this
/// compilation unit** counts. Library members reach the backend through
/// emitters of their own that already drop the value where they produce it
/// (`Using.resource`, `Breaks.catchBreak`, `ArrayOps`), and popping it a
/// second time underflows the stack. A callee declared to return `Unit`
/// returns `V` and leaves nothing at all either way.
pub(crate) fn unit_stat_leaves_ref(tree: &Tree, st: &SymbolTable) -> bool {
    match &tree.kind {
        TreeKind::Typed { expr, .. } | TreeKind::Block { expr, .. } => {
            unit_stat_leaves_ref(expr, st)
        }
        TreeKind::Apply { fun, .. } => {
            let f = peel_fun(fun);
            leaves_ref_sym(f.sym, st, false)
        }
        // A **nilary** `def` has no argument list, so calling it builds a bare
        // `Select` with no `Apply` above it and the arm above never sees it:
        // `b.get` on a `Box[Unit]`, where `trait Box[A] { def get: A }`, still
        // invokes `get()Ljava/lang/Object;`. Discarded without a `pop` the
        // reference survives into the next stackmap frame, and the first
        // branch after it (a `try`, an `if`, a `while`) is
        // `VerifyError: Inconsistent stackmap frames`. Straight-line code got
        // away with it, which is why this went unnoticed.
        TreeKind::Select { .. } | TreeKind::Ident { .. } => leaves_ref_sym(tree.sym, st, true),
        _ => false,
    }
}

/// The shared test behind both arms of `unit_stat_leaves_ref`: a member this
/// compilation unit defines whose *declared* result is not `Unit`, so its JVM
/// signature returns a value even where the use site's type is `Unit`.
///
/// `only_methods` is set for the bare-`Select` arm. An `Apply` may be a call
/// through a function-typed `val` (`Function1.apply` erases to
/// `(Object)Object` too), but a bare `Select` of a `val` is a field read whose
/// descriptor `pop_if_value` already covers from the tree's own type, and
/// popping it a second time would underflow the stack.
pub(crate) fn leaves_ref_sym(sym: SymbolId, st: &SymbolTable, only_methods: bool) -> bool {
    if sym.is_none() {
        return false;
    }
    let s = st.get(sym);
    if only_methods && s.kind != SymKind::Method {
        return false;
    }
    if !matches!(s.intrinsic, Intrinsic::None) || !owner_defined_in_source(st, s.owner) {
        return false;
    }
    match &s.ty {
        Type::Method { ret, .. } | Type::Function { ret, .. } => {
            !matches!(ret.as_ref(), Type::Unit | Type::NoType | Type::Nothing)
        }
        _ => false,
    }
}

/// True when the code just emitted for `tree` left a reference on the stack
/// even though `tree`'s Scala type is `Unit`: `pf.apply(x)` for a
/// `PartialFunction[A, Unit]` still invokes `(Object)Object`. Discarding such
/// an expression has to `pop`, or a later `goto` merges two stack heights.
///
/// Deliberately narrow. Most `Unit` expressions leave nothing behind, and the
/// intrinsics that do erase through `Object` (`Breaks.catchBreak`,
/// `Using.resource`) already drop the value where they are emitted.
pub(crate) fn unit_call_leaves_ref(tree: &Tree, st: &SymbolTable) -> bool {
    match &tree.kind {
        TreeKind::Typed { expr, .. } | TreeKind::Block { expr, .. } => {
            unit_call_leaves_ref(expr, st)
        }
        TreeKind::Apply { fun, .. } => match &peel_fun(fun).kind {
            TreeKind::Select { qual, name } => {
                name == "apply" && is_partial_function_ty(st, &qual.ty)
            }
            _ => false,
        },
        _ => false,
    }
}

pub(crate) fn emit_predef_nyi(asm: &mut Assembler) {
    load_predef_module(asm);
    asm.invokevirtual("scala/Predef$", "???", "()Lscala/runtime/Nothing$;");
}

pub(crate) fn gen_predef_println(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    args: &[Tree],
    newline: bool,
) {
    if args.is_empty() {
        load_predef_module(asm);
        if newline {
            asm.invokevirtual("scala/Predef$", "println", "()V");
        } else {
            asm.aconst_null();
            asm.invokevirtual("scala/Predef$", "print", "(Ljava/lang/Object;)V");
        }
        return;
    }
    // Evaluate the argument first so a comparison's branch target is not
    // sitting under `MODULE$` (Java 6 inference verifier / later StackMap).
    let a = &args[0];
    gen_expr(asm, frame, ctx, a);
    if is_unit_like(&a.ty) {
        // nsc Predef.println(x: Any) prints BoxedUnit / null, not a fake "()".
        if !unit_leaves_boxed_ref(a, ctx.st) {
            emit_boxed_unit(asm);
        }
    } else if is_jvm_primitive(&a.ty) {
        emit_box(asm, &a.ty);
    }
    load_predef_module(asm);
    asm.swap();
    let name = if newline { "println" } else { "print" };
    asm.invokevirtual("scala/Predef$", name, "(Ljava/lang/Object;)V");
}

pub(crate) fn gen_predef_poly(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    args: &[Tree],
    result_ty: &Type,
    name: &str,
) {
    let Some(a) = args.first() else {
        load_predef_module(asm);
        asm.aconst_null();
        asm.invokevirtual(
            "scala/Predef$",
            name,
            "(Ljava/lang/Object;)Ljava/lang/Object;",
        );
        if is_unit_like(result_ty) {
            asm.pop();
        }
        return;
    };
    gen_expr(asm, frame, ctx, a);
    if is_unit_like(&a.ty) {
        if unit_leaves_boxed_ref(a, ctx.st) {
            asm.checkcast(BOXED_UNIT);
        } else {
            emit_boxed_unit(asm);
        }
    } else if is_jvm_primitive(&a.ty) {
        emit_box(asm, &a.ty);
    }
    load_predef_module(asm);
    asm.swap();
    asm.invokevirtual(
        "scala/Predef$",
        name,
        "(Ljava/lang/Object;)Ljava/lang/Object;",
    );
    if is_unit_like(result_ty) {
        asm.pop();
    }
}

pub(crate) fn gen_predef_assert_require(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    args: &[Tree],
    is_assert: bool,
) {
    let Some(cond) = args.first() else {
        return;
    };
    gen_expr(asm, frame, ctx, cond);
    let name = if is_assert { "assert" } else { "require" };
    if let Some(msg) = args.get(1) {
        gen_expr(asm, frame, ctx, msg);
        load_predef_module(asm);
        // cond, msg, MODULE$ → MODULE$, cond, msg
        asm.dup_x2();
        asm.pop();
        asm.invokevirtual("scala/Predef$", name, "(ZLscala/Function0;)V");
    } else {
        load_predef_module(asm);
        asm.swap();
        asm.invokevirtual("scala/Predef$", name, "(Z)V");
    }
}

pub(crate) fn is_list_unapply_seq(st: &SymbolTable, uid: SymbolId) -> bool {
    let s = st.get(uid);
    s.name == "unapplySeq" && is_list_module_owner(&class_internal(st, s.owner))
}

pub(crate) fn is_arrow_assoc_arrow(ctx: &EmitCtx, fun: &Tree) -> bool {
    if fun.name() != Some("->") {
        return false;
    }
    if fun.sym.is_none() {
        return true;
    }
    class_internal(ctx.st, ctx.st.get(fun.sym).owner).contains("ArrowAssoc")
}

pub(crate) fn gen_tuple2_arrow(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    fun: &Tree,
    args: &[Tree],
) {
    asm.new_obj("scala/Tuple2");
    asm.dup();
    gen_receiver(asm, frame, ctx, fun);
    if let Some(a) = args.first() {
        gen_expr(asm, frame, ctx, a);
        if is_jvm_primitive(&a.ty) {
            emit_box(asm, &a.ty);
        }
    } else {
        asm.aconst_null();
    }
    asm.invokespecial(
        "scala/Tuple2",
        "<init>",
        "(Ljava/lang/Object;Ljava/lang/Object;)V",
    );
}

pub(crate) fn gen_assert_require(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    args: &[Tree],
    is_assert: bool,
) {
    let Some(cond) = args.first() else {
        return;
    };
    gen_expr(asm, frame, ctx, cond);
    let ok = asm.fresh_label();
    asm.ifne(ok);
    let cls = if is_assert {
        "java/lang/AssertionError"
    } else {
        "java/lang/IllegalArgumentException"
    };
    asm.new_obj(cls);
    asm.dup();
    if let Some(msg) = args.get(1) {
        gen_expr(asm, frame, ctx, msg);
        if matches!(&msg.ty, Type::Function { .. }) {
            asm.invokeinterface("scala/Function0", "apply", "()Ljava/lang/Object;");
        } else if is_jvm_primitive(&msg.ty) {
            emit_box(asm, &msg.ty);
        }
        asm.invokevirtual("java/lang/Object", "toString", "()Ljava/lang/String;");
        if is_assert {
            asm.invokespecial(cls, "<init>", "(Ljava/lang/Object;)V");
        } else {
            asm.invokespecial(cls, "<init>", "(Ljava/lang/String;)V");
        }
    } else if is_assert {
        asm.invokespecial(cls, "<init>", "()V");
    } else {
        asm.ldc_string("requirement failed");
        asm.invokespecial(cls, "<init>", "(Ljava/lang/String;)V");
    }
    asm.athrow();
    asm.mark(ok);
}

pub(crate) fn emit_box(asm: &mut Assembler, ty: &Type) {
    emit_box_inner(asm, &ty.widen_constant())
}

pub(crate) fn emit_box_inner(asm: &mut Assembler, ty: &Type) {
    match ty {
        Type::Int => {
            asm.invokestatic("java/lang/Integer", "valueOf", "(I)Ljava/lang/Integer;");
        }
        Type::Boolean => {
            asm.invokestatic("java/lang/Boolean", "valueOf", "(Z)Ljava/lang/Boolean;");
        }
        Type::Byte => {
            asm.invokestatic("java/lang/Byte", "valueOf", "(B)Ljava/lang/Byte;");
        }
        Type::Short => {
            asm.invokestatic("java/lang/Short", "valueOf", "(S)Ljava/lang/Short;");
        }
        Type::Long => {
            asm.invokestatic("java/lang/Long", "valueOf", "(J)Ljava/lang/Long;");
        }
        Type::Double => {
            asm.invokestatic("java/lang/Double", "valueOf", "(D)Ljava/lang/Double;");
        }
        Type::Char => {
            asm.invokestatic("java/lang/Character", "valueOf", "(C)Ljava/lang/Character;");
        }
        Type::Float => {
            asm.invokestatic("java/lang/Float", "valueOf", "(F)Ljava/lang/Float;");
        }
        // `()` boxes to the `BoxedUnit` singleton, never to `null`: that is
        // what makes `println(x: Any)` print `()` and `(x: Any) == ()` true.
        // The private runtime emits its own `scala/runtime/BoxedUnit`, so both
        // modes agree here.
        Type::Unit | Type::NoType => {
            emit_boxed_unit(asm);
        }
        _ => {}
    }
}

pub(crate) fn emit_unbox(asm: &mut Assembler, ty: &Type) {
    emit_unbox_inner(asm, &ty.widen_constant())
}

pub(crate) fn emit_unbox_inner(asm: &mut Assembler, ty: &Type) {
    match ty {
        Type::Int => {
            asm.checkcast("java/lang/Integer");
            asm.invokevirtual("java/lang/Integer", "intValue", "()I");
        }
        Type::Boolean => {
            asm.checkcast("java/lang/Boolean");
            asm.invokevirtual("java/lang/Boolean", "booleanValue", "()Z");
        }
        Type::Byte => {
            asm.checkcast("java/lang/Byte");
            asm.invokevirtual("java/lang/Byte", "byteValue", "()B");
        }
        Type::Short => {
            asm.checkcast("java/lang/Short");
            asm.invokevirtual("java/lang/Short", "shortValue", "()S");
        }
        Type::Long => {
            asm.checkcast("java/lang/Long");
            asm.invokevirtual("java/lang/Long", "longValue", "()J");
        }
        Type::Double => {
            asm.checkcast("java/lang/Double");
            asm.invokevirtual("java/lang/Double", "doubleValue", "()D");
        }
        Type::Char => {
            asm.checkcast("java/lang/Character");
            asm.invokevirtual("java/lang/Character", "charValue", "()C");
        }
        Type::Float => {
            asm.checkcast("java/lang/Float");
            asm.invokevirtual("java/lang/Float", "floatValue", "()F");
        }
        Type::String => {
            asm.checkcast("java/lang/String");
        }
        Type::Class { .. } | Type::ModuleRef(_) => {
            // leave as Object; checkcast when we have an internal name
        }
        Type::Unit | Type::NoType => {
            asm.pop();
        }
        _ => {}
    }
}

/// Boxed wrapper class for a primitive (`Int` -> `java/lang/Integer`, ...),
/// used by `asInstanceOf`/`isInstanceOf` against an `Any`-erased (`Object`)
/// receiver that may hold a boxed primitive.
pub(crate) fn boxed_internal_name(ty: &Type) -> Option<&'static str> {
    match ty {
        Type::Int => Some("java/lang/Integer"),
        Type::Boolean => Some("java/lang/Boolean"),
        Type::Byte => Some("java/lang/Byte"),
        Type::Short => Some("java/lang/Short"),
        Type::Long => Some("java/lang/Long"),
        Type::Double => Some("java/lang/Double"),
        Type::Char => Some("java/lang/Character"),
        Type::Float => Some("java/lang/Float"),
        _ => None,
    }
}

/// `recv.asInstanceOf[target]`: the receiver is already on the stack (`Object`,
/// since `Any` erases there). Primitives unbox from their boxed wrapper;
/// `String`/class types `checkcast`; unbounded/erased targets (`Any`,
/// `AnyRef`, a type parameter, ...) need no cast since they are already
/// `Object`-compatible post-erasure — matching nsc, which likewise only
/// emits `checkcast` against a type with real runtime class information.
pub(crate) fn emit_as_instance_of(asm: &mut Assembler, ctx: &EmitCtx, target: &Type) {
    if boxed_internal_name(target).is_some() {
        emit_unbox(asm, target);
        return;
    }
    match target {
        Type::String => asm.checkcast("java/lang/String"),
        Type::Unit | Type::NoType => {
            // The result is a `Unit` *expression*, so it leaves nothing on the
            // stack -- the receiver was only evaluated for its effect. nsc
            // drops the cast entirely and materialises `BoxedUnit.UNIT` at
            // whatever value position the result goes to.
            asm.pop();
        }
        _ => {
            if let Some(cn) = checkcast_internal(ctx.st, target) {
                if cn != "java/lang/Object" {
                    asm.checkcast(&cn);
                }
            }
        }
    }
}

/// `e.asInstanceOf[T]` where `e`'s own type is a JVM primitive.
///
/// `emit_as_instance_of` reads its receiver as an `Object` (that is what `Any`
/// erases to), so it can neither be handed an `int` nor left to `checkcast`
/// one. nsc's erasure settles this case before the cast exists: a primitive
/// cast to another primitive is a *numeric conversion* (`i.asInstanceOf[Long]`
/// is `i2l`, and to the same type it is nothing at all), and a primitive cast
/// to any reference type is a box. Only the box needs the cast that follows,
/// and only when the target names a real class.
///
/// Returns whether the whole cast has been emitted here.
///
/// slick's `StatementInvoker.iteratorTo` is
/// `results(maxRows).fold(r => new CloseableIterator.Single[R](r.asInstanceOf[R]), identity)`,
/// where `r` is the `Int` of an `Either[Int, …]`: `new Single(int)` against a
/// constructor taking `Object` is a `VerifyError`, and it is the first thing
/// every `.result` in `slick_run.sh` reaches.
pub(crate) fn emit_prim_qualifier_cast(asm: &mut Assembler, src: &Type, target: &Type) -> bool {
    let src = src.widen_constant();
    if !is_jvm_primitive(&src) || is_unit_like(&src) {
        return false;
    }
    if let (Some(from), Some(to)) = (prim_desc_char(&src), prim_desc_char(target)) {
        // `Boolean` is not a numeric type: nsc leaves such a cast alone, and
        // `emit_num_conv`'s codes do not describe it either.
        if from == to {
            return true;
        }
        if from != 'Z' && to != 'Z' {
            emit_num_conv(asm, &format!("{from}{to}"));
            return true;
        }
        return true;
    }
    emit_box(asm, &src);
    // The target is a reference type; `emit_as_instance_of` still owes it a
    // `checkcast` when it names a class (`3.asInstanceOf[Integer]`).
    false
}

/// The JVM descriptor letter of a primitive, as `emit_num_conv` spells it.
pub(crate) fn prim_desc_char(ty: &Type) -> Option<char> {
    match ty.widen_constant() {
        Type::Boolean => Some('Z'),
        Type::Byte => Some('B'),
        Type::Short => Some('S'),
        Type::Char => Some('C'),
        Type::Int => Some('I'),
        Type::Long => Some('J'),
        Type::Float => Some('F'),
        Type::Double => Some('D'),
        _ => None,
    }
}

/// `recv.isInstanceOf[target]`: the receiver is already on the stack.
/// Primitives check against the boxed wrapper; erased/unbounded targets fall
/// back to `java/lang/Object` (always true for a non-null receiver), which is
/// also what nsc's own erasure does for an unchecked type parameter.
pub(crate) fn emit_is_instance_of(asm: &mut Assembler, ctx: &EmitCtx, target: &Type) {
    if let Some(bn) = boxed_internal_name(target) {
        asm.instanceof(bn);
        return;
    }
    let cn = match target {
        Type::String => "java/lang/String".to_string(),
        // `x.isInstanceOf[Unit]` is `instanceof scala/runtime/BoxedUnit`,
        // which is what nsc emits.
        t if erases_to_boxed_unit(t) => BOXED_UNIT.to_string(),
        _ => checkcast_internal(ctx.st, target).unwrap_or_else(|| "java/lang/Object".to_string()),
    };
    asm.instanceof(&cn);
}

/// The arity of the `FunctionN` a *class* type inherits, if any. Structural
/// `Type::Function` is handled directly everywhere; this is for the classes
/// that extend one (`<:<`, `=:=`, `PartialFunction`, `trait Mono extends
/// (Int => String)`).
pub(crate) fn inherited_function_arity(st: &SymbolTable, ty: &Type) -> Option<usize> {
    if !matches!(ty, Type::Class { .. }) {
        return None;
    }
    st.base_type_seq(ty).into_iter().find_map(|b| match b {
        Type::Function { params, .. } => Some(params.len()),
        Type::Class { sym, args } => {
            let n: usize = st
                .get(sym)
                .jvm_name
                .strip_prefix("scala/Function")?
                .parse()
                .ok()?;
            (args.len() == n + 1).then_some(n)
        }
        _ => None,
    })
}

pub(crate) fn gen_function_apply(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    fun: &Tree,
    args: &[Tree],
    result_ty: &Type,
) {
    gen_expr(asm, frame, ctx, fun);
    let n = match &fun.ty {
        Type::Function { params, .. } => params.len(),
        _ => args.len(),
    };
    let param_tys = match &fun.ty {
        Type::Function { params, .. } => params.clone(),
        _ => args.iter().map(|a| a.ty.clone()).collect(),
    };
    for (i, a) in args.iter().enumerate() {
        gen_expr(asm, frame, ctx, a);
        let pty = param_tys.get(i).unwrap_or(&a.ty);
        if is_jvm_primitive(pty) || is_jvm_primitive(&a.ty) {
            emit_box(asm, &a.ty);
        }
    }
    let iface = format!("scala/Function{n}");
    let mut desc = String::from("(");
    for _ in 0..n {
        desc.push_str("Ljava/lang/Object;");
    }
    desc.push_str(")Ljava/lang/Object;");
    asm.invokeinterface(&iface, "apply", &desc);
    if is_jvm_primitive(result_ty) {
        emit_unbox(asm, result_ty);
    } else if matches!(result_ty, Type::String) {
        emit_unbox(asm, result_ty);
    } else if let Some(cn) = checkcast_internal(ctx.st, result_ty) {
        // `FunctionN.apply` erases to `Object`; every reference result owes a
        // cast. This used to name only `Class` and `Function` (the latter for
        // `f.curried(3)(4)`), so a *tuple* result went uncast: slick's
        // `def dealias(n: Node)(f: Node => (Node, Mappings)): (Node, Mappings)`
        // ends in `case n => f(n)` and the method failed verification.
        if cn != "java/lang/Object" {
            asm.checkcast(&cn);
        }
    }
}

pub(crate) fn is_jvm_primitive(ty: &Type) -> bool {
    matches!(
        ty.widen_constant(),
        Type::Int
            | Type::Long
            | Type::Double
            | Type::Boolean
            | Type::Char
            | Type::Float
            | Type::Byte
            | Type::Short
            | Type::Unit
    )
}
