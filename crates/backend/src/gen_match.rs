//! Pattern-match lowering: `match` itself, the `tableswitch`/`lookupswitch`
//! form, constructor and `unapply` / `unapplySeq` patterns, and the tests,
//! casts and bindings a single pattern compiles to.

use crate::code::Assembler;
use crate::gen::*;
use scala_rs_parser::{Flags, Lit, SymbolId, Tree, TreeKind, Type};
use scala_rs_typer::SymKind;

pub(crate) fn gen_match(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    selector: &Tree,
    cases: &[scala_rs_parser::CaseDef],
    result_ty: &Type,
) {
    gen_expr(asm, frame, ctx, selector);
    let sel_sort = jvm_sort(&selector.ty);
    let tmp = frame.alloc_tmp(sel_sort);
    store(asm, tmp, sel_sort);
    if gen_int_switch(asm, frame, ctx, selector, cases, result_ty, tmp, sel_sort) {
        return;
    }
    let end = asm.fresh_label();
    if let Some(n) = join_class_of(ctx.st, result_ty) {
        asm.set_join_class(end, &n);
    }
    for c in cases {
        let fail = asm.fresh_label();
        gen_pattern(asm, frame, ctx, &c.pat, tmp, sel_sort, fail);
        if !c.guard.is_empty() {
            gen_expr(asm, frame, ctx, &c.guard);
            asm.ifeq(fail);
        }
        if is_unit_like(result_ty) {
            gen_stat(asm, frame, ctx, &c.body);
        } else {
            gen_expr(asm, frame, ctx, &c.body);
        }
        asm.goto(end);
        asm.mark(fail);
    }
    throw_match_error(asm, ctx, &selector.ty, tmp);
    asm.mark(end);
}

pub(crate) enum SwitchPat {
    Key(i32),
    Default,
}

pub(crate) fn switch_pat_key(pat: &Tree) -> Option<SwitchPat> {
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

pub(crate) fn peel_type_annot<'a>(ty: &'a Type) -> &'a Type {
    match ty {
        Type::Annotated { tpe, .. } => peel_type_annot(tpe),
        t => t,
    }
}

pub(crate) fn gen_int_switch(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    selector: &Tree,
    cases: &[scala_rs_parser::CaseDef],
    result_ty: &Type,
    tmp: u16,
    sel_sort: JvmSort,
) -> bool {
    if sel_sort != JvmSort::Int {
        return false;
    }
    let core = peel_type_annot(&selector.ty);
    if !matches!(core, Type::Int | Type::Char) {
        return false;
    }
    let mut keys: Vec<(i32, usize)> = Vec::new();
    let mut default_idx: Option<usize> = None;
    for (i, c) in cases.iter().enumerate() {
        if !c.guard.is_empty() {
            return false;
        }
        match switch_pat_key(&c.pat) {
            Some(SwitchPat::Key(k)) => {
                if keys.iter().any(|(ek, _)| *ek == k) {
                    return false;
                }
                keys.push((k, i));
            }
            Some(SwitchPat::Default) => {
                if default_idx.is_some() {
                    return false;
                }
                default_idx = Some(i);
            }
            None => return false,
        }
    }
    if keys.is_empty() {
        return false;
    }
    let mut case_labs: Vec<crate::code::Label> = Vec::new();
    for _ in cases {
        case_labs.push(asm.fresh_label());
    }
    let miss = asm.fresh_label();
    let end = asm.fresh_label();
    if let Some(n) = join_class_of(ctx.st, result_ty) {
        asm.set_join_class(end, &n);
    }
    let def_lab = default_idx.map(|i| case_labs[i]).unwrap_or(miss);
    load(asm, tmp, sel_sort);
    let lo = keys.iter().map(|(k, _)| *k).min().unwrap();
    let hi = keys.iter().map(|(k, _)| *k).max().unwrap();
    let n = keys.len() as i64;
    let space = hi as i64 - lo as i64 + 1;
    if space <= n * 2 && space <= 4096 {
        let mut table: Vec<crate::code::Label> = Vec::new();
        for k in lo..=hi {
            let lab = keys
                .iter()
                .find(|(ek, _)| *ek == k)
                .map(|(_, i)| case_labs[*i])
                .unwrap_or(def_lab);
            table.push(lab);
        }
        asm.tableswitch(def_lab, lo, hi, &table);
    } else {
        let mut pairs: Vec<(i32, crate::code::Label)> =
            keys.iter().map(|(k, i)| (*k, case_labs[*i])).collect();
        pairs.sort_by_key(|(k, _)| *k);
        asm.lookupswitch(def_lab, &pairs);
    }
    for (i, c) in cases.iter().enumerate() {
        asm.mark(case_labs[i]);
        if let TreeKind::Ident { .. } | TreeKind::Bind { .. } = &c.pat.kind {
            if matches!(switch_pat_key(&c.pat), Some(SwitchPat::Default)) {
                gen_pattern(asm, frame, ctx, &c.pat, tmp, sel_sort, miss);
            }
        }
        if is_unit_like(result_ty) {
            gen_stat(asm, frame, ctx, &c.body);
        } else {
            gen_expr(asm, frame, ctx, &c.body);
        }
        asm.goto(end);
    }
    if default_idx.is_none() {
        asm.mark(miss);
        throw_match_error(asm, ctx, &selector.ty, tmp);
    }
    asm.mark(end);
    true
}

/// The class `unapply`'s first parameter is *declared* with, read off the
/// descriptor the call will use.
///
/// The descriptor is the only reliable source: a type-parameter parameter
/// erases to `Object` however its `Type` reads, and casting to the `Type`'s
/// own name would name a class the method never sees. `None` means the
/// parameter is `Object` or a primitive -- nothing to cast to.
pub(crate) fn unapply_param_class(ctx: &EmitCtx, uid: SymbolId) -> Option<String> {
    if uid.is_none() {
        return None;
    }
    // `+:` / `:+` take a `SeqOps`, which their prelude signature does not say.
    if let Some(d) = crate::gen_invoke::cons_extractor_desc(
        &class_internal(ctx.st, ctx.st.get(uid).owner),
        ctx.st.get(uid).name.as_str(),
    ) {
        let inner = d.strip_prefix("(L")?.split(';').next()?;
        return Some(inner.to_string());
    }
    let desc = method_desc_boxed(ctx.st, uid, ctx.boxed_vars);
    let params = desc.strip_prefix('(')?.split(')').next()?.to_string();
    if let Some(rest) = params.strip_prefix('L') {
        let n = rest.split(';').next()?;
        return (n != "java/lang/Object").then(|| n.to_string());
    }
    if params.starts_with('[') {
        // `[` runs to the end of one descriptor; the first parameter's is a
        // prefix of `params`, and an array descriptor is its own cast target.
        let bytes = params.as_bytes();
        let mut i = 0;
        while i < bytes.len() && bytes[i] == b'[' {
            i += 1;
        }
        if i < bytes.len() && bytes[i] == b'L' {
            let end = params[i..].find(';')? + i + 1;
            return Some(params[..end].to_string());
        }
        return Some(params[..=i.min(bytes.len() - 1)].to_string());
    }
    None
}

/// A `case class` pattern that reads the constructor fields directly.
///
/// Shared by the `Apply` pattern arm and by the synthetic `unapply` of a
/// case-class companion: nsc emits a real `unapply` there, we do not, and a
/// pattern that named only the *first* parameter list of a multi-clause case
/// class (slick's `case TableNode(_, _, i, b)`, whose class has a second
/// `(val profileTable: Any)` clause) took the extractor path and died with
/// `NoSuchMethodError: TableNode$.unapply`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn gen_ctor_fields_pattern(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    pat: &Tree,
    args: &[Tree],
    class_id: SymbolId,
    tmp: u16,
    sel_sort: JvmSort,
    fail: crate::code::Label,
) {
    // A value class is held *unboxed* wherever its static type says so, and a
    // box is always a reference -- so a scrutinee of primitive sort here is
    // already the underlying value. `case W(x)` then has nothing to test (the
    // class is final and the static type names it) and nothing to read: it
    // binds the sub-pattern to the scrutinee itself. This is what nsc emits
    // for the same source (`iload; istore`, no `instanceof`). Without it the
    // `instanceof` / `checkcast` / `getfield` below ran against an `int` local
    // -- `VerifyError: Bad local variable type`.
    if !matches!(sel_sort, JvmSort::Ref | JvmSort::Void)
        && !class_id.is_none()
        && ctx.st.is_value_class(class_id)
        && args.len() == 1
    {
        gen_pattern(asm, frame, ctx, &args[0], tmp, sel_sort, fail);
        return;
    }
    let jvm = if class_id.is_none() {
        pat.name().unwrap_or("java/lang/Object").to_string()
    } else {
        class_internal(ctx.st, class_id)
    };
    load(asm, tmp, JvmSort::Ref);
    asm.instanceof(&jvm);
    asm.ifeq(fail);
    let fields = if class_id.is_none() {
        Vec::new()
    } else {
        ctx.st.get(class_id).ctor_fields.clone()
    };
    for (i, a) in args.iter().enumerate() {
        if let Some(fid) = fields.get(i) {
            let fs = ctx.st.get(*fid);
            let fname = fs.name.clone();
            let fty = fs.ty.clone();
            // The *field* is a value position (`Unit` is
            // `BoxedUnit` there); the accessor's *result* is not
            // (`Unit` is `V`). They are only the same descriptor for
            // every other type.
            let fdesc = jvm_desc_val(ctx.st, &fty);
            let acc_desc = format!("(){}", jvm_desc(ctx.st, &fty));
            load(asm, tmp, JvmSort::Ref);
            asm.checkcast(&jvm);
            // A case class's field is private with a public accessor
            // (`scala.util.Failure.exception`), so reading the field
            // is an `IllegalAccessError`. Call the accessor whenever
            // the class has one -- which is what nsc emits, and what
            // our own case classes have too. `jvm_name` names it when
            // the accessor is spelled differently.
            let acc = ctx.st.get(*fid).jvm_name.clone();
            // Only call an accessor we know exists: a library field's
            // accessor is not always spelled like the field
            // (`$colon$colon.tl` is read through `next$access$1`), so
            // guessing the name produced `NoSuchMethodError`.
            let acc = if !acc.is_empty() {
                Some(acc)
            } else if !ctx.st.source_classes.contains(&class_id)
                && has_nullary_accessor(ctx.st, class_id, &fname)
            {
                Some(fname.clone())
            } else {
                None
            };
            match acc {
                Some(a) => asm.invokevirtual(&jvm, &a, &acc_desc),
                None => emit_getfield(asm, &jvm, &fname, &fdesc),
            }
            // A field declared as a type parameter erases to Object, so
            // `case Some(x)` on an `Option[Int]` must unbox before it
            // binds. A sub-pattern that *tests* must not be narrowed
            // here, though: casting to it turned
            // `case Some((s, _: TableNode))` on a plain `Node`, and
            // `case P(v) :: t` on every non-`P` head, into a
            // `ClassCastException` instead of a failed match.
            let sort = if reads_erased_value(ctx, a) {
                // The test reads the field as it stands.
                jvm_sort(&fty)
            } else {
                if fdesc == "Ljava/lang/Object;" {
                    emit_from_erased_object(asm, ctx.st, &a.ty);
                }
                jvm_sort(&a.ty)
            };
            bind_subpattern(asm, frame, ctx, a, sort, fail);
        } else {
            report_ctx_error(ctx, pat.span, "pattern arity");
            throw_runtime(asm, "pattern arity");
        }
    }
}

pub(crate) fn gen_unapply_pattern(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    pat: &Tree,
    fun: &Tree,
    args: &[Tree],
    tmp: u16,
    sel_sort: JvmSort,
    fail: crate::code::Label,
) {
    let uid = if pat.sym.is_none() { fun.sym } else { pat.sym };
    // A `case class`'s companion `unapply` is synthesized as a *symbol* with
    // no body; nothing emits the method, so calling it is a
    // `NoSuchMethodError`. Read the constructor fields directly instead, which
    // is what the `Apply` form of the same pattern already does. This is the
    // path a pattern takes when it names only the first of several parameter
    // lists (slick's `case TableNode(_, _, i, b)`).
    if !uid.is_none() {
        let s = ctx.st.get(uid);
        if s.name == "unapply" && s.flags.contains(Flags::CASE) {
            if let Some(cls) = companion_case_class(ctx.st, s.owner) {
                gen_ctor_fields_pattern(asm, frame, ctx, pat, args, cls, tmp, sel_sort, fail);
                return;
            }
        }
    }
    let ret_bool = !uid.is_none() && matches!(ctx.st.get(uid).ty.result(), Type::Boolean);
    let param0 = if uid.is_none() {
        None
    } else {
        match &ctx.st.get(uid).ty {
            Type::Method { paramss, .. } => paramss.first().and_then(|ps| ps.first()).cloned(),
            Type::Function { params, .. } => params.first().cloned(),
            _ => None,
        }
    };
    // An `unapply` on a plain class is an instance method: the extractor is
    // the *value* `Lib.Cast`, not the object that holds it.
    let owner = if uid.is_none() {
        SymbolId::NONE
    } else {
        ctx.st.get(uid).owner
    };
    let is_seq = !uid.is_none() && ctx.st.get(uid).name == "unapplySeq";
    let shape = if uid.is_none() {
        SeqPatShape::List
    } else {
        seq_pat_shape(ctx.st, uid)
    };
    // `case Seq(a, b)` against an `Any` scrutinee has to *test* first: the
    // wrapper's extension methods throw on anything that is not a sequence
    // (`Array$UnapplySeqWrapper$.lengthCompare$extension` reflects on the
    // argument). scalac emits `instanceof` / `ScalaRunTime.isArray` for the
    // same reason, and only when the static type does not already say so.
    if is_seq && sel_sort == JvmSort::Ref {
        let known = match shape {
            // `is_sub_type` is lenient about an `Array[A]` whose element is a
            // bare type parameter; only a scrutinee that already *is* an array
            // may skip the test.
            SeqPatShape::Array => matches!(pat.ty, Type::Array(_)),
            _ => param0
                .as_ref()
                .is_some_and(|p| ctx.st.is_sub_type(&pat.ty, p)),
        };
        if !known {
            match shape {
                // `Array$` only exists with the jar, so this arm is only
                // reachable there; the private runtime never sees it.
                SeqPatShape::Array => {
                    if ctx.library_abi {
                        asm.getstatic(
                            "scala/runtime/ScalaRunTime$",
                            "MODULE$",
                            "Lscala/runtime/ScalaRunTime$;",
                        );
                        load(asm, tmp, sel_sort);
                        asm.iconst(1);
                        asm.invokevirtual(
                            "scala/runtime/ScalaRunTime$",
                            "isArray",
                            "(Ljava/lang/Object;I)Z",
                        );
                        asm.ifeq(fail);
                    }
                }
                SeqPatShape::List | SeqPatShape::SeqOps => {
                    load(asm, tmp, sel_sort);
                    asm.instanceof(&seq_pat_test_class(ctx, param0.as_ref()));
                    asm.ifeq(fail);
                }
            }
        }
    }
    // An extractor pattern never matches `null`: nsc guards the call with
    // `ifnull` rather than handing the extractor a null to reason about, so
    // `case Foo(a) => … case null => …` reaches the `null` case. The
    // sequence shapes above already tested with `instanceof`, but a scrutinee
    // whose static type made that test redundant still needs this.
    if sel_sort == JvmSort::Ref {
        load(asm, tmp, sel_sort);
        asm.ifnull(fail);
    }
    // `unapply` takes its own parameter type, and the value in `tmp` does not
    // always have it. A *nested* extractor is now handed the value as the
    // source left it (see `reads_erased_value`), which is `Object` whenever the
    // source field is erased -- and the pattern's own type may be wider than
    // the extractor accepts (`case Some(Two(a, b))` on an `Option[Any]`). nsc
    // emits `instanceof` / `ifeq` / `checkcast` in that second case and falls
    // through to the next case; without the test the call did not even verify.
    // A user-written `unapplySeq` takes its own parameter type too: slick's
    // `FunctionSymbol.unapplySeq(a: Apply)` was handed the still-erased
    // `Object` a `Tuple2._1` read produced, and the method failed
    // verification ("Type 'java/lang/Object' is not assignable to
    // 'slick/ast/Apply'"). The `instanceof` test above already ran for the
    // sequence shapes; only the cast was missing, so compute the class for
    // those too and let the `checkcast` below use it.
    let param_class = (sel_sort == JvmSort::Ref && shape != SeqPatShape::Array)
        .then(|| unapply_param_class(ctx, uid))
        .flatten();
    if !is_seq {
        if let Some(cls) = &param_class {
            let known = param0
                .as_ref()
                .is_some_and(|p| ctx.st.is_sub_type(&pat.ty, p));
            if !known {
                load(asm, tmp, sel_sort);
                asm.instanceof(cls);
                asm.ifeq(fail);
            }
        }
    }
    if !owner.is_none() && !is_module_class(ctx.st, owner) {
        gen_expr(asm, frame, ctx, fun);
    } else if !owner.is_none() {
        // The `unapply` being called belongs to `owner`, so the receiver is
        // `owner`'s singleton -- not whatever the *name* in the pattern is
        // owned by. Those differ when the extractor is reached through a
        // stable value that aliases the object: slick's
        // `object syntax { val :: = HCons }`, imported into `HList`, made
        // `case (h1 :: t1, x)` emit `getstatic syntax$.MODULE$` under
        // `invokevirtual HCons$.unapply` and the JVM threw the whole method
        // out (`VerifyError: Bad type on operand stack … 'syntax$' is not
        // assignable to 'HCons$'`).
        load_module_instance(asm, ctx, module_class_id(ctx.st, owner));
    } else {
        gen_receiver(asm, frame, ctx, fun);
    }
    load(asm, tmp, sel_sort);
    if is_seq && ctx.library_abi && shape == SeqPatShape::SeqOps {
        // The forwarder's parameter is `SeqOps`; the scrutinee's static type
        // may be anything the test above let through.
        asm.checkcast(&seq_pat_test_class(ctx, param0.as_ref()));
    } else if let Some(cls) = &param_class {
        asm.checkcast(cls);
    } else if sel_sort == JvmSort::Ref {
        // A primitive-parameter `unapply` reached through an erased field:
        // the descriptor wants the unboxed value.
        if let Some(p) = param0.as_ref().filter(|p| is_jvm_primitive(p)) {
            emit_unbox(asm, p);
        }
    }
    if let Some(p) = &param0 {
        if !is_jvm_primitive(p) && sel_sort != JvmSort::Ref && sel_sort != JvmSort::Void {
            let pty = match sel_sort {
                JvmSort::Int => Type::Int,
                JvmSort::Long => Type::Long,
                JvmSort::Double => Type::Double,
                JvmSort::Float => Type::Float,
                _ => Type::Any,
            };
            emit_box(asm, &pty);
        }
    }
    if uid.is_none() {
        asm.pop();
        asm.pop();
        report_ctx_error(ctx, pat.span, "unresolved unapply");
        throw_runtime(asm, "unresolved unapply");
        return;
    }
    invoke_method(asm, ctx, uid, None);
    if ret_bool {
        asm.ifeq(fail);
        return;
    }
    if is_seq && ctx.library_abi {
        match shape {
            // scala-library `List.unapplySeq` is identity on SeqOps, not Option.
            SeqPatShape::List if is_list_unapply_seq(ctx.st, uid) => {
                gen_unapply_seq_bind(asm, frame, ctx, args, fail);
                return;
            }
            SeqPatShape::SeqOps => {
                gen_unapply_wrapper_bind(asm, frame, ctx, args, fail, SeqPatShape::SeqOps);
                return;
            }
            SeqPatShape::Array => {
                gen_unapply_wrapper_bind(asm, frame, ctx, args, fail, SeqPatShape::Array);
                return;
            }
            SeqPatShape::List => {}
        }
    }
    asm.dup();
    asm.invokevirtual("scala/Option", "isEmpty", "()Z");
    let nonempty = asm.fresh_label();
    asm.ifeq(nonempty);
    asm.pop();
    asm.goto(fail);
    asm.mark(nonempty);
    asm.invokevirtual("scala/Option", "get", "()Ljava/lang/Object;");
    if is_seq {
        let payload = if ctx.library_abi {
            user_unapply_seq_shape(ctx, uid)
        } else {
            SeqPatShape::List
        };
        match payload {
            SeqPatShape::List => gen_unapply_seq_bind(asm, frame, ctx, args, fail),
            other => gen_unapply_wrapper_bind(asm, frame, ctx, args, fail, other),
        }
        return;
    }
    if args.len() <= 1 {
        if let Some(a) = args.first() {
            let sort = coerce_subpattern(asm, ctx, a);
            bind_subpattern(asm, frame, ctx, a, sort, fail);
        } else {
            asm.pop();
        }
    } else {
        // The tuple goes in a local, not on the stack: a sub-pattern that
        // tests jumps to `fail` from inside `bind_subpattern`, and the `dup`ed
        // tuple left there made the two paths disagree about the stack
        // (`VerifyError: Inconsistent stackmap frames`, for
        // `case P(v) ~ _` on a user-defined infix extractor).
        let tuple = format!("scala/Tuple{}", args.len());
        asm.checkcast(&tuple);
        let slot = frame.alloc_tmp(JvmSort::Ref);
        store(asm, slot, JvmSort::Ref);
        for (i, a) in args.iter().enumerate() {
            let fname = format!("_{}", i + 1);
            load(asm, slot, JvmSort::Ref);
            if args.len() == 2 {
                // `scala.Tuple2`'s two fields are public, and so are the
                // private runtime's; every wider tuple keeps them private
                // behind an accessor, which is what nsc calls.
                asm.getfield(&tuple, &fname, "Ljava/lang/Object;");
            } else {
                asm.invokevirtual(&tuple, &fname, "()Ljava/lang/Object;");
            }
            let sort = coerce_subpattern(asm, ctx, a);
            bind_subpattern(asm, frame, ctx, a, sort, fail);
        }
    }
}

/// Prepare the erased value on top of the stack for `pat`, and report the sort
/// it is left in.
///
/// A `_: T` (or `x: T`) sub-pattern **tests**: it needs the reference the
/// `instanceof` reads, and `gen_pattern`'s own `Typed` arm unboxes or casts it
/// once the test has passed. Unboxing here left an `int` in the local that arm
/// then `aload`ed -- `val (n: Int, s: String) = …` was
/// `VerifyError: Bad local variable type`.
pub(crate) fn coerce_subpattern(asm: &mut Assembler, ctx: &EmitCtx, pat: &Tree) -> JvmSort {
    if reads_erased_value(ctx, pat) {
        return JvmSort::Ref;
    }
    if is_jvm_primitive(&pat.ty) {
        emit_unbox(asm, &pat.ty);
    } else {
        emit_pattern_cast(asm, ctx, &pat.ty);
    }
    jvm_sort(&pat.ty)
}

/// Does this sub-pattern want the extracted value exactly as the source left
/// it, rather than narrowed to the sub-pattern's own type?
///
/// Every sub-pattern that **tests** does. nsc casts an extracted value to the
/// *source's* static type (`$colon$colon.head` on a `List[C]` is
/// `checkcast C`) and only then emits `instanceof P` / `ifeq` / `checkcast P`
/// for the sub-pattern; narrowing to the sub-pattern's type up front turns a
/// non-match into a `ClassCastException`. That is what `case P(v) :: t` did to
/// every list whose head was not a `P`, and what `case Some(1)` on an
/// `Option[Any]` did to every non-`Int` element.
///
/// A binding `x` is the one shape that really does need the narrowing: its
/// local is typed at the pattern's type. `x @ p` does not -- `gen_pattern`'s
/// `Bind` arm re-reads the value and narrows it itself, after `p`'s test.
pub(crate) fn reads_erased_value(ctx: &EmitCtx, pat: &Tree) -> bool {
    match &pat.kind {
        // `_: T` and `P(...)` / `Foo(...)` test before they narrow.
        TreeKind::Typed { .. } | TreeKind::Apply { .. } | TreeKind::UnApply { .. } => true,
        // A constant or stable-id pattern compares; nsc compares the boxed
        // constant with `BoxesRunTime.equals` rather than unboxing first.
        TreeKind::Literal { .. } | TreeKind::Select { .. } => true,
        TreeKind::Ident { name } => !is_binding_ident(ctx, pat, name),
        // Nothing to narrow, and `bind_subpattern` pops at the source's sort.
        TreeKind::Wildcard | TreeKind::Empty => true,
        TreeKind::Bind { .. } => true,
        _ => false,
    }
}

/// A lowercase (or `_`-leading) identifier pattern **binds**; `Nil`, `Q` and
/// other stable ids must be compared.
pub(crate) fn is_binding_ident(ctx: &EmitCtx, pat: &Tree, name: &str) -> bool {
    // `stable_pat` is the type checker's answer: the name resolved to a value
    // and the pattern is a comparison. It has to be asked first, because a
    // resolved `val` and a fresh pattern variable are both `SymKind::Term`
    // and the test below would call every constant pattern a binding.
    !pat.stable_pat
        && (name
            .chars()
            .next()
            .is_some_and(|c| c.is_lowercase() || c == '_')
            || pat.sym.is_none()
            || ctx.st.get(pat.sym).kind == SymKind::Term)
}

/// Put a constant pattern's value on the stack in the *scrutinee's* JVM
/// representation: boxed when the scrutinee is a reference (`case 1 =>` on an
/// `Any`), widened when the scrutinee is a wider primitive (`case 1 =>` on a
/// `Long`). Without this the comparison below saw two different sorts and the
/// verifier rejected the method.
pub(crate) fn emit_pattern_operand(asm: &mut Assembler, ty: &Type, sel_sort: JvmSort) {
    let ty = ty.widen_constant();
    if !is_jvm_primitive(&ty) {
        return;
    }
    if sel_sort == JvmSort::Ref {
        emit_box(asm, &ty);
        return;
    }
    let want = match sel_sort {
        JvmSort::Int => Type::Int,
        JvmSort::Long => Type::Long,
        JvmSort::Float => Type::Float,
        JvmSort::Double => Type::Double,
        _ => return,
    };
    widen_primitive(asm, &ty, &want);
}

/// Compare a constant pattern with the scrutinee, jumping to `fail` when they
/// differ. The pattern's value is pushed *first* and the scrutinee second,
/// which is the order nsc emits: with the scrutinee as the receiver,
/// `case "a" =>` threw a `NullPointerException` on a `null` scrutinee instead
/// of falling through to the next case.
pub(crate) fn emit_pattern_eq_jump(
    asm: &mut Assembler,
    ctx: &EmitCtx,
    sel_sort: JvmSort,
    fail: crate::code::Label,
) {
    match sel_sort {
        JvmSort::Int => asm.if_icmpne(fail),
        // A `Long`/`Float`/`Double` scrutinee used to have both operands
        // popped and the case taken unconditionally: `case 1L =>` matched
        // every `Long`.
        JvmSort::Long => {
            asm.lcmp();
            asm.ifne(fail);
        }
        JvmSort::Float => {
            asm.fcmpl();
            asm.ifne(fail);
        }
        JvmSort::Double => {
            asm.dcmpl();
            asm.ifne(fail);
        }
        JvmSort::Ref => {
            if ctx.library_abi {
                // What nsc emits: it also equates a boxed `1` with a boxed
                // `1L`, which `Integer.equals` does not.
                asm.invokestatic(
                    "scala/runtime/BoxesRunTime",
                    "equals",
                    "(Ljava/lang/Object;Ljava/lang/Object;)Z",
                );
            } else {
                asm.invokevirtual("java/lang/Object", "equals", "(Ljava/lang/Object;)Z");
            }
            asm.ifeq(fail);
        }
        // A `Unit` scrutinee has one value, so a `Unit` pattern always matches.
        JvmSort::Void => {}
    }
}

/// `Option.get` yields `Object`; a bound pattern variable of a narrower
/// reference type needs the cast the verifier expects.
pub(crate) fn emit_pattern_cast(asm: &mut Assembler, ctx: &EmitCtx, ty: &Type) {
    if is_jvm_primitive(ty) {
        return;
    }
    let target = match ty {
        Type::Array(_) => jvm_desc(ctx.st, ty),
        Type::NoType | Type::Error | Type::Any | Type::AnyRef => return,
        // `type_jvm_name` answers `java/lang/Object` for a structural tuple
        // type, i.e. no cast at all. A `(TermSymbol, Node)` really is a
        // `scala/Tuple2` at run time, and a binder of that type read out of an
        // erased extractor needs the cast before its `_1` / `_2`. slick's
        // `case StructNode(ConstArray(ch, _*)) => ch._2` had
        // `apply$extension(SeqOps, I)Object` feeding a
        // `getfield scala/Tuple2._2` (`VerifyError` in
        // `MergeToComprehensions`, which is every `groupBy` and every join).
        Type::Tuple(ts) if !ts.is_empty() => format!("scala/Tuple{}", ts.len()),
        _ => type_jvm_name(ctx.st, ty),
    };
    if target.is_empty() || target == "java/lang/Object" {
        return;
    }
    asm.checkcast(&target);
}

/// `case Seq(a, b)` / `case Vector(a, rest @ _*)` / `case Array(a, b)`.
///
/// Reads elements by index instead of walking a cons list, which is what
/// scalac does and the only thing that works for a `Vector` or an
/// `ArraySeq` reached through `Seq`. The value on the stack is whatever
/// `unapplySeq` returned -- the identity `SeqOps` for a `SeqFactory`, the
/// array itself for `scala.Array` -- and the wrapper's extension methods take
/// exactly that.
pub(crate) fn gen_unapply_wrapper_bind(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    args: &[Tree],
    fail: crate::code::Label,
    shape: SeqPatShape,
) {
    let (wrapper, self_desc) = match shape {
        SeqPatShape::Array => (ARRAY_WRAPPER, "Ljava/lang/Object;"),
        _ => (SEQOPS_WRAPPER, "Lscala/collection/SeqOps;"),
    };
    let module_desc = format!("L{wrapper};");
    if shape != SeqPatShape::Array {
        asm.checkcast("scala/collection/SeqOps");
    }
    let slot = frame.alloc_tmp(JvmSort::Ref);
    store(asm, slot, JvmSort::Ref);
    // `_*` is last: `type_pattern` rejects it anywhere else.
    let has_star = args.last().is_some_and(is_star_pat);
    let fixed = if has_star { args.len() - 1 } else { args.len() };

    asm.getstatic(wrapper, "MODULE$", &module_desc);
    load(asm, slot, JvmSort::Ref);
    asm.iconst(fixed as i32);
    asm.invokevirtual(
        wrapper,
        "lengthCompare$extension",
        &format!("({self_desc}I)I"),
    );
    if has_star {
        asm.iflt(fail);
    } else {
        asm.ifne(fail);
    }

    for (i, a) in args.iter().take(fixed).enumerate() {
        asm.getstatic(wrapper, "MODULE$", &module_desc);
        load(asm, slot, JvmSort::Ref);
        asm.iconst(i as i32);
        asm.invokevirtual(
            wrapper,
            "apply$extension",
            &format!("({self_desc}I)Ljava/lang/Object;"),
        );
        let sort = coerce_subpattern(asm, ctx, a);
        bind_subpattern(asm, frame, ctx, a, sort, fail);
    }

    if let Some(star) = args.last().filter(|a| is_star_pat(a)) {
        asm.getstatic(wrapper, "MODULE$", &module_desc);
        load(asm, slot, JvmSort::Ref);
        asm.iconst(fixed as i32);
        asm.invokevirtual(
            wrapper,
            "drop$extension",
            &format!("({self_desc}I)Lscala/collection/immutable/Seq;"),
        );
        if !reads_erased_value(ctx, star) {
            emit_pattern_cast(asm, ctx, &star.ty);
        }
        bind_subpattern(asm, frame, ctx, star, JvmSort::Ref, fail);
    }
}

pub(crate) fn gen_unapply_seq_bind(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    args: &[Tree],
    fail: crate::code::Label,
) {
    asm.checkcast("scala/collection/immutable/List");
    let list_slot = frame.alloc_tmp(JvmSort::Ref);
    store(asm, list_slot, JvmSort::Ref);
    let mut saw_star = false;
    for a in args {
        if is_star_pat(a) {
            load(asm, list_slot, JvmSort::Ref);
            bind_subpattern(asm, frame, ctx, a, JvmSort::Ref, fail);
            saw_star = true;
            break;
        }
        load(asm, list_slot, JvmSort::Ref);
        asm.invokevirtual("scala/collection/immutable/List", "isEmpty", "()Z");
        asm.ifne(fail);
        load(asm, list_slot, JvmSort::Ref);
        emit_list_head(asm, ctx);
        let sort = coerce_subpattern(asm, ctx, a);
        bind_subpattern(asm, frame, ctx, a, sort, fail);
        load(asm, list_slot, JvmSort::Ref);
        emit_list_tail(asm, ctx);
        store(asm, list_slot, JvmSort::Ref);
    }
    if !saw_star {
        load(asm, list_slot, JvmSort::Ref);
        asm.invokevirtual("scala/collection/immutable/List", "isEmpty", "()Z");
        asm.ifeq(fail);
    }
}

pub(crate) fn emit_list_head(asm: &mut Assembler, ctx: &EmitCtx) {
    if ctx.library_abi {
        // `head` is on LinearSeqOps; List itself has no `head()Object` method.
        asm.invokeinterface(
            "scala/collection/LinearSeqOps",
            "head",
            "()Ljava/lang/Object;",
        );
    } else {
        asm.invokevirtual(
            "scala/collection/immutable/List",
            "head",
            "()Ljava/lang/Object;",
        );
    }
}

pub(crate) fn emit_list_tail(asm: &mut Assembler, ctx: &EmitCtx) {
    if ctx.library_abi {
        asm.invokevirtual(
            "scala/collection/immutable/List",
            "tail",
            "()Lscala/collection/LinearSeq;",
        );
        asm.checkcast("scala/collection/immutable/List");
    } else {
        asm.invokevirtual(
            "scala/collection/immutable/List",
            "tail",
            "()Lscala/collection/immutable/List;",
        );
    }
}

pub(crate) fn gen_pattern(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    pat: &Tree,
    tmp: u16,
    sel_sort: JvmSort,
    fail: crate::code::Label,
) {
    match &pat.kind {
        TreeKind::Wildcard | TreeKind::Empty => {}
        TreeKind::Ident { name } => {
            if is_binding_ident(ctx, pat, name) {
                load(asm, tmp, sel_sort);
                let sort = jvm_sort(&pat.ty);
                let slot = if pat.sym.is_none() {
                    frame.alloc_tmp(sort)
                } else if let Some((s, _)) = frame.get(pat.sym) {
                    s
                } else {
                    frame.alloc(pat.sym, sort)
                };
                store(asm, slot, sort);
            } else {
                // The stable id goes on the stack first, the scrutinee second:
                // see `emit_pattern_eq_jump`.
                gen_ident(asm, frame, ctx, pat);
                emit_pattern_operand(asm, &pat.ty, sel_sort);
                load(asm, tmp, sel_sort);
                emit_pattern_eq_jump(asm, ctx, sel_sort, fail);
            }
        }
        TreeKind::Literal { lit } => {
            // SLS 8.1.1: the `null` pattern is a *reference* comparison.
            // `x.equals(null)` threw a `NullPointerException` on the one
            // scrutinee the case exists to catch. A `()` pattern is *not* one
            // of these: `()` boxes to `BoxedUnit.UNIT` in both modes now, so
            // it compares by value and does not also match `null`.
            let by_reference = matches!(lit, Lit::Null);
            if by_reference {
                if sel_sort == JvmSort::Ref {
                    load(asm, tmp, sel_sort);
                    asm.ifnonnull(fail);
                } else {
                    // A primitive scrutinee is never null.
                    asm.goto(fail);
                }
            } else {
                gen_literal(asm, lit);
                emit_pattern_operand(asm, &pat.ty, sel_sort);
                load(asm, tmp, sel_sort);
                emit_pattern_eq_jump(asm, ctx, sel_sort, fail);
            }
        }
        TreeKind::Select { .. } => {
            gen_expr(asm, frame, ctx, pat);
            emit_pattern_operand(asm, &pat.ty, sel_sort);
            load(asm, tmp, sel_sort);
            emit_pattern_eq_jump(asm, ctx, sel_sort, fail);
        }
        TreeKind::Apply { args, .. } => {
            let class_id = if pat.sym.is_none() {
                ctx.st.class_sym_of(&pat.ty).unwrap_or(SymbolId::NONE)
            } else {
                pat.sym
            };
            gen_ctor_fields_pattern(asm, frame, ctx, pat, args, class_id, tmp, sel_sort, fail);
        }
        TreeKind::UnApply { fun, args } => {
            gen_unapply_pattern(asm, frame, ctx, pat, fun, args, tmp, sel_sort, fail);
        }
        TreeKind::Bind { body, .. } => {
            // `case n @ N(v, _)` binds `n` at the pattern's *own* type, not at
            // the scrutinee's: run the inner pattern's test first, then narrow
            // what the test proved. Storing the raw scrutinee left `n` typed
            // `T` in the frame and reading `N`'s fields off it was a
            // `VerifyError`; `case n @ (_: Int)` on an `Any` stored a reference
            // in an `int` slot the same way. nsc emits the same order:
            // `instanceof`, `checkcast`, `astore`.
            gen_pattern(asm, frame, ctx, body, tmp, sel_sort, fail);
            let sort = jvm_sort(&pat.ty);
            load(asm, tmp, sel_sort);
            if sel_sort == JvmSort::Ref {
                emit_from_erased_object(asm, ctx.st, &pat.ty);
            }
            let slot = if pat.sym.is_none() {
                frame.alloc_tmp(sort)
            } else if let Some((s, _)) = frame.get(pat.sym) {
                s
            } else {
                frame.alloc(pat.sym, sort)
            };
            store(asm, slot, sort);
        }
        TreeKind::Typed { expr, .. } => {
            // `case x: Meters` on an `Any` tests for the boxed value class and
            // reads the underlying back out of it; erasure stamped the class on
            // the ascription node (`mark_value_class_patterns`).
            if !pat.sym.is_none()
                && ctx.st.is_value_class(pat.sym)
                && sel_sort == JvmSort::Ref
                && !matches!(pat.ty, Type::Class { .. })
            {
                let internal = class_internal(ctx.st, pat.sym);
                load(asm, tmp, JvmSort::Ref);
                asm.instanceof(&internal);
                asm.ifeq(fail);
                let binds = !matches!(expr.kind, TreeKind::Wildcard | TreeKind::Empty);
                if binds {
                    let field = ctx.st.get(pat.sym).ctor_fields.first().copied();
                    let (fname, fty) = match field {
                        Some(f) => (ctx.st.get(f).name.clone(), ctx.st.get(f).ty.clone()),
                        None => (String::new(), Type::Any),
                    };
                    load(asm, tmp, JvmSort::Ref);
                    asm.checkcast(&internal);
                    asm.getfield(&internal, &fname, &jvm_desc(ctx.st, &fty));
                    let want = jvm_sort(&fty);
                    let narrowed = frame.alloc_tmp(want);
                    store(asm, narrowed, want);
                    gen_pattern(asm, frame, ctx, expr, narrowed, want, fail);
                }
                return;
            }
            if sel_sort != JvmSort::Ref {
                // The scrutinee is already unboxed, so its class is settled
                // and the ascription only names it again (`case Point(x: Int)`
                // on an `Int` field). There is nothing to test and nothing to
                // load as a reference.
                gen_pattern(asm, frame, ctx, expr, tmp, sel_sort, fail);
                return;
            }
            // A type pattern never matches `null` (SLS 8.1.2), which is what
            // `instanceof` says on its own -- so the test is emitted even when
            // the type erases to `Object` (`case x: Any`, `case x: AnyRef`,
            // `case a: A`). Skipping it there let `null` reach a case that
            // scalac's own `instanceof java/lang/Object` rules out.
            // `type_jvm_name` reports `Object` for an array, which tested
            // nothing and left `case a: Array[Int]` reading `arraylength` off
            // an `Object`; `instanceof` takes the array descriptor directly.
            let jvm = match pat.ty.widen_constant() {
                Type::Array(_) => jvm_desc(ctx.st, &pat.ty),
                _ => type_jvm_name(ctx.st, &pat.ty),
            };
            let jvm = if jvm.is_empty() {
                "java/lang/Object".to_string()
            } else {
                jvm
            };
            load(asm, tmp, JvmSort::Ref);
            asm.instanceof(&jvm);
            asm.ifeq(fail);
            // A compound type pattern has to test *every* parent.
            // `type_jvm_name` names only the first one, so
            // `case _: TA with TB` matched a value that is merely a `TA`.
            if let Type::Refined { parents, .. } = pat.ty.widen_constant() {
                for p in &parents {
                    let n = type_jvm_name(ctx.st, p);
                    if n.is_empty() || n == "java/lang/Object" || n == jvm {
                        continue;
                    }
                    load(asm, tmp, JvmSort::Ref);
                    asm.instanceof(&n);
                    asm.ifeq(fail);
                }
            }
            // `case i: Int` / `case s: String` narrows an `Object` scrutinee,
            // so the bound value is unboxed or cast before it is stored.
            let want = jvm_sort(&pat.ty);
            let binds = !matches!(expr.kind, TreeKind::Wildcard | TreeKind::Empty);
            if binds && (want != sel_sort || jvm != "java/lang/Object") {
                load(asm, tmp, sel_sort);
                emit_from_erased_object(asm, ctx.st, &pat.ty);
                let narrowed = frame.alloc_tmp(want);
                store(asm, narrowed, want);
                gen_pattern(asm, frame, ctx, expr, narrowed, want, fail);
            } else {
                gen_pattern(asm, frame, ctx, expr, tmp, sel_sort, fail);
            }
        }
        TreeKind::Alternative { trees } => {
            // `case _: Int | _: String =>`: the first alternative that matches
            // wins; only when they all fail does the case fail.
            let ok = asm.fresh_label();
            for alt in trees {
                let next = asm.fresh_label();
                gen_pattern(asm, frame, ctx, alt, tmp, sel_sort, next);
                asm.goto(ok);
                asm.mark(next);
            }
            asm.goto(fail);
            asm.mark(ok);
        }
        _ => {}
    }
}

/// Bind the value on top of the stack to `pat`. `sort` is the sort that value
/// actually has -- which is *not* always `jvm_sort(&pat.ty)`: a `_: T` test
/// sub-pattern keeps the erased reference so `gen_pattern` can `instanceof` it.
pub(crate) fn bind_subpattern(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    pat: &Tree,
    sort: JvmSort,
    fail: crate::code::Label,
) {
    // field value is on the stack
    match &pat.kind {
        TreeKind::Wildcard | TreeKind::Empty => {
            pop_sort(asm, sort);
        }
        // A lowercase identifier binds; `Nil` and other stable ids must be
        // compared, so they fall through to `gen_pattern` below.
        TreeKind::Ident { name } if is_binding_ident(ctx, pat, name) => {
            let slot = if pat.sym.is_none() {
                frame.alloc_tmp(sort)
            } else if let Some((s, _)) = frame.get(pat.sym) {
                s
            } else {
                frame.alloc(pat.sym, sort)
            };
            store(asm, slot, sort);
        }
        _ => {
            // Nested patterns (`case h :: Nil`, `case Some(Some(x))`) need the
            // full matcher, which reads its value from a local.
            let tmp = frame.alloc_tmp(sort);
            store(asm, tmp, sort);
            gen_pattern(asm, frame, ctx, pat, tmp, sort, fail);
        }
    }
}
