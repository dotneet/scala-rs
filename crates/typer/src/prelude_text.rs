//! String / StringBuilder / Range / numeric enrichment additions.
//!
//! Kept separate from `prelude.rs` to avoid merge conflicts with sibling
//! agents working on other prelude slices. Only a single call
//! (`crate::prelude_text::install`) is wired into `install_prelude`.
use crate::prelude::{fn1, fn2, method, type_param};
use crate::symbol::{Intrinsic, SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

pub fn install(st: &mut SymbolTable, library_abi: bool) {
    add_string_extra(st);
    if library_abi {
        add_string_builder_full(st);
        add_range_ops(st);
        add_math_object(st);
        add_numeric_enrichment_extra(st);
    }
}

/// Find a direct member of `owner` with the given `name` and `kind`.
fn find(st: &SymbolTable, owner: SymbolId, name: &str, kind: SymKind) -> SymbolId {
    st.lookup_member(owner, name)
        .into_iter()
        .find(|&id| st.get(id).kind == kind)
        .unwrap_or(SymbolId::NONE)
}

/// `java.lang.String` methods missing from `add_string_members` in prelude.rs.
/// These back onto the real JDK `java.lang.String` class, which is present at
/// runtime regardless of `--scala-library`/`--no-scala-library`, so they are
/// registered unconditionally (matching `startsWith`/`endsWith`/`indexOf`).
fn add_string_extra(st: &mut SymbolTable) {
    let c = st.string_sym;
    method(st, c, "trim", vec![], Type::String, Intrinsic::None);
    method(
        st,
        c,
        "substring",
        vec![Type::Int],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        c,
        "substring",
        vec![Type::Int, Type::Int],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        c,
        "lastIndexOf",
        vec![Type::String],
        Type::Int,
        Intrinsic::None,
    );
    method(
        st,
        c,
        "replace",
        vec![Type::Char, Type::Char],
        Type::String,
        Intrinsic::None,
    );
    // Real descriptor takes two `CharSequence`s.
    let m = method(
        st,
        c,
        "replace",
        vec![Type::String, Type::String],
        Type::String,
        Intrinsic::None,
    );
    st.get_mut(m).jvm_name =
        "(Ljava/lang/CharSequence;Ljava/lang/CharSequence;)Ljava/lang/String;".into();
    // Real descriptor takes `CharSequence`, not `String`; a `String` argument
    // is assignable, so override the computed descriptor to match the ABI.
    let m = method(
        st,
        c,
        "contains",
        vec![Type::String],
        Type::Boolean,
        Intrinsic::None,
    );
    st.get_mut(m).jvm_name = "(Ljava/lang/CharSequence;)Z".into();
    method(
        st,
        c,
        "equalsIgnoreCase",
        vec![Type::String],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        c,
        "matches",
        vec![Type::String],
        Type::Boolean,
        Intrinsic::None,
    );
    method(st, c, "strip", vec![], Type::String, Intrinsic::None);
    method(st, c, "stripLeading", vec![], Type::String, Intrinsic::None);
    method(
        st,
        c,
        "stripTrailing",
        vec![],
        Type::String,
        Intrinsic::None,
    );
    method(st, c, "isBlank", vec![], Type::Boolean, Intrinsic::None);
    method(
        st,
        c,
        "repeat",
        vec![Type::Int],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        c,
        "compareTo",
        vec![Type::String],
        Type::Int,
        Intrinsic::None,
    );
}

/// `scala.collection.mutable.StringBuilder` extras: constructors, the full
/// `append` overload set, `insert`, `deleteCharAt`, `setLength`, `reverse`,
/// `clear`, `isEmpty`, `length`, `++=`, `result`.
///
/// Reuses the *same* class symbol `add_string_builder` (prelude.rs) already
/// registered under `scala.collection.mutable`, then also aliases it as a
/// direct member of `scala` so bare `StringBuilder` resolves too (matching
/// `scala.StringBuilder`'s real alias) -- a second, distinct class symbol
/// would desync from the fully-qualified spelling and confuse the
/// owner-string-keyed dispatch in gen.rs's `is_stdlib_stringbuilder` block.
fn add_string_builder_full(st: &mut SymbolTable) {
    let mutp = crate::classpath::ensure_package(st, "scala/collection/mutable");
    let sb = find(st, mutp, "StringBuilder", SymKind::Class);
    if sb.is_none() {
        return;
    }
    if st.lookup_member(st.scala_pkg, "StringBuilder").is_empty() {
        st.get_mut(st.scala_pkg).members.push(sb);
    }
    let sb_t = Type::Class {
        sym: sb,
        args: vec![],
    };

    // Constructors: (), (Int), (String)
    for params in [vec![], vec![Type::Int], vec![Type::String]] {
        method(st, sb, "<init>", params, sb_t.clone(), Intrinsic::None);
    }

    // append overloads: one per primitive plus String/Any, all returning
    // StringBuilder directly at the JVM level (no erasure on this family).
    for p in [
        Type::Any,
        Type::String,
        Type::Char,
        Type::Int,
        Type::Long,
        Type::Double,
        Type::Float,
        Type::Boolean,
        Type::Byte,
        Type::Short,
    ] {
        method(st, sb, "append", vec![p], sb_t.clone(), Intrinsic::None);
    }

    // `+=` mirrors `addOne` (Char), which is a concrete non-erased override.
    method(
        st,
        sb,
        "+=",
        vec![Type::Char],
        sb_t.clone(),
        Intrinsic::None,
    );
    // `++=` appends a whole String (addAll(String), also non-erased).
    method(
        st,
        sb,
        "++=",
        vec![Type::String],
        sb_t.clone(),
        Intrinsic::None,
    );

    method(st, sb, "toString", vec![], Type::String, Intrinsic::None);
    method(st, sb, "result", vec![], Type::String, Intrinsic::None);
    method(st, sb, "length", vec![], Type::Int, Intrinsic::None);
    method(st, sb, "isEmpty", vec![], Type::Boolean, Intrinsic::None);
    method(st, sb, "nonEmpty", vec![], Type::Boolean, Intrinsic::None);
    method(st, sb, "clear", vec![], Type::Unit, Intrinsic::None);
    method(
        st,
        sb,
        "setLength",
        vec![Type::Int],
        Type::Unit,
        Intrinsic::None,
    );
    method(
        st,
        sb,
        "deleteCharAt",
        vec![Type::Int],
        sb_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        sb,
        "insert",
        vec![Type::Int, Type::Any],
        sb_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        sb,
        "insert",
        vec![Type::Int, Type::String],
        sb_t.clone(),
        Intrinsic::None,
    );
    // `reverse` is inherited from IndexedSeqOps and erased to Object; the
    // generic invoke fallback checkcasts back to StringBuilder using this
    // declared return type.
    method(st, sb, "reverse", vec![], sb_t.clone(), Intrinsic::None);
    method(
        st,
        sb,
        "charAt",
        vec![Type::Int],
        Type::Char,
        Intrinsic::None,
    );
    method(
        st,
        sb,
        "apply",
        vec![Type::Int],
        Type::Char,
        Intrinsic::None,
    );
}

fn add_range_ops(st: &mut SymbolTable) {
    let range = find(st, st.scala_pkg, "Range", SymKind::Class);
    if range.is_none() {
        return;
    }
    let range_t = Type::Class {
        sym: range,
        args: vec![],
    };
    let idx = find(st, st.scala_pkg, "IndexedSeq", SymKind::Class);
    let idx_int = Type::Class {
        sym: idx,
        args: vec![Type::Int],
    };
    let list_int = Type::Class {
        sym: st.list_sym,
        args: vec![Type::Int],
    };
    let vector = find(st, st.scala_pkg, "Vector", SymKind::Class);
    let vector_int = Type::Class {
        sym: vector,
        args: vec![Type::Int],
    };
    let tuple2 = find(st, st.scala_pkg, "Tuple2", SymKind::Class);
    let tuple2_int_int = Type::Class {
        sym: tuple2,
        args: vec![Type::Int, Type::Int],
    };
    // `Range.withFilter(...).map/flatMap(...)` returns `IndexedSeq[Int]`, not
    // `Range` (mirrors `Range.map`'s own erasure/return type below).
    let with_filter = find(st, st.scala_pkg, "WithFilter", SymKind::Class);
    let wf_t = Type::Class {
        sym: with_filter,
        args: vec![Type::Int, idx_int.clone()],
    };

    // `withFilter` unblocks `for (x <- 1 to 3 if p) yield ...`.
    method(
        st,
        range,
        "withFilter",
        vec![fn1(Type::Int, Type::Boolean)],
        wf_t,
        Intrinsic::None,
    );

    // Range-returning, non-erased at the JVM level (concrete overrides).
    method(
        st,
        range,
        "take",
        vec![Type::Int],
        range_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        range,
        "drop",
        vec![Type::Int],
        range_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        range,
        "takeRight",
        vec![Type::Int],
        range_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        range,
        "dropRight",
        vec![Type::Int],
        range_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        range,
        "takeWhile",
        vec![fn1(Type::Int, Type::Boolean)],
        range_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        range,
        "dropWhile",
        vec![fn1(Type::Int, Type::Boolean)],
        range_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        range,
        "reverse",
        vec![],
        range_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        range,
        "by",
        vec![Type::Int],
        range_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        range,
        "slice",
        vec![Type::Int, Type::Int],
        range_t.clone(),
        Intrinsic::None,
    );

    // Fixed-Int-valued, non-erased.
    method(st, range, "isEmpty", vec![], Type::Boolean, Intrinsic::None);
    method(
        st,
        range,
        "nonEmpty",
        vec![],
        Type::Boolean,
        Intrinsic::None,
    );
    method(st, range, "size", vec![], Type::Int, Intrinsic::None);
    method(st, range, "head", vec![], Type::Int, Intrinsic::None);
    method(st, range, "last", vec![], Type::Int, Intrinsic::None);
    method(
        st,
        range,
        "contains",
        vec![Type::Int],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        range,
        "splitAt",
        vec![Type::Int],
        Type::Tuple(vec![range_t.clone(), range_t.clone()]),
        Intrinsic::None,
    );

    // Erased to Object at the JVM level (bridge from SeqOps/IterableOnceOps);
    // the generic invoke fallback checkcasts to these declared types.
    method(
        st,
        range,
        "filter",
        vec![fn1(Type::Int, Type::Boolean)],
        idx_int.clone(),
        Intrinsic::None,
    );
    method(
        st,
        range,
        "filterNot",
        vec![fn1(Type::Int, Type::Boolean)],
        idx_int.clone(),
        Intrinsic::None,
    );
    method(
        st,
        range,
        "map",
        vec![fn1(Type::Int, Type::Any)],
        idx_int.clone(),
        Intrinsic::None,
    );
    method(
        st,
        range,
        "flatMap",
        vec![fn1(Type::Int, Type::Any)],
        idx_int.clone(),
        Intrinsic::None,
    );
    method(
        st,
        range,
        "zipWithIndex",
        vec![],
        Type::Class {
            sym: idx,
            args: vec![tuple2_int_int],
        },
        Intrinsic::None,
    );
    method(st, range, "toList", vec![], list_int, Intrinsic::None);
    method(st, range, "toVector", vec![], vector_int, Intrinsic::None);
    method(
        st,
        range,
        "toArray",
        vec![],
        Type::Array(Box::new(Type::Int)),
        Intrinsic::None,
    );

    // `IterableOnceOps` default methods (generic `foldLeft`/`sum`/etc). The
    // named term params (not just `.ty`) are required for lambda-arg
    // inference, matching `add_array_ops_folds`'s `foldLeft`.
    let m = method(st, range, "foldLeft", vec![], Type::Unit, Intrinsic::None);
    let b = type_param(st, m, "B");
    let tb = Type::TypeParam(b);
    let z = st.alloc("z", m, SymKind::Term, Flags::PARAM, "");
    st.get_mut(z).ty = tb.clone();
    let op = st.alloc("op", m, SymKind::Term, Flags::PARAM, "");
    st.get_mut(op).ty = fn2(tb.clone(), Type::Int, tb.clone());
    st.get_mut(m).tparams = vec![b];
    st.get_mut(m).params = vec![z, op];
    st.get_mut(m).paramss = vec![vec![z], vec![op]];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![
            vec![tb.clone()],
            vec![fn2(tb.clone(), Type::Int, tb.clone())],
        ],
        ret: Box::new(tb),
    };
    let m = method(st, range, "foldRight", vec![], Type::Unit, Intrinsic::None);
    let b = type_param(st, m, "B");
    let tb = Type::TypeParam(b);
    let z = st.alloc("z", m, SymKind::Term, Flags::PARAM, "");
    st.get_mut(z).ty = tb.clone();
    let op = st.alloc("op", m, SymKind::Term, Flags::PARAM, "");
    st.get_mut(op).ty = fn2(Type::Int, tb.clone(), tb.clone());
    st.get_mut(m).tparams = vec![b];
    st.get_mut(m).params = vec![z, op];
    st.get_mut(m).paramss = vec![vec![z], vec![op]];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![
            vec![tb.clone()],
            vec![fn2(Type::Int, tb.clone(), tb.clone())],
        ],
        ret: Box::new(tb),
    };
    method(st, range, "sum", vec![], Type::Int, Intrinsic::None);
    method(st, range, "product", vec![], Type::Int, Intrinsic::None);
    method(st, range, "min", vec![], Type::Int, Intrinsic::None);
    method(st, range, "max", vec![], Type::Int, Intrinsic::None);
    method(
        st,
        range,
        "exists",
        vec![fn1(Type::Int, Type::Boolean)],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        range,
        "forall",
        vec![fn1(Type::Int, Type::Boolean)],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        range,
        "count",
        vec![fn1(Type::Int, Type::Boolean)],
        Type::Int,
        Intrinsic::None,
    );
}

/// Register a `scala.math` package-object function: a plain `method()` plus
/// `Flags::STATIC` so `invoke_method` emits `invokestatic` (there is no
/// receiver -- `scala.math` itself has no runtime value) against the
/// redirected `scala/math/package` owner (see `invoke_method` in gen.rs).
fn smethod(st: &mut SymbolTable, owner: SymbolId, name: &str, params: Vec<Type>, ret: Type) {
    let m = method(st, owner, name, params, ret, Intrinsic::None);
    st.get_mut(m).flags = st.get(m).flags.with(Flags::STATIC);
}

/// `scala.math` package-object functions (`abs`/`max`/`min`/`pow`/`sqrt`/
/// `floor`/`ceil`/`round`/`random`). These live on the real static-forwarder
/// class `scala.math.package`, so this is `library_abi`-only.
fn add_math_object(st: &mut SymbolTable) {
    let math = crate::classpath::ensure_package(st, "scala/math");
    for ty in [Type::Int, Type::Long, Type::Float, Type::Double] {
        smethod(st, math, "abs", vec![ty.clone()], ty.clone());
        smethod(st, math, "max", vec![ty.clone(), ty.clone()], ty.clone());
        smethod(st, math, "min", vec![ty.clone(), ty.clone()], ty.clone());
        smethod(st, math, "signum", vec![ty.clone()], ty);
    }
    smethod(
        st,
        math,
        "pow",
        vec![Type::Double, Type::Double],
        Type::Double,
    );
    smethod(st, math, "sqrt", vec![Type::Double], Type::Double);
    smethod(st, math, "cbrt", vec![Type::Double], Type::Double);
    smethod(st, math, "floor", vec![Type::Double], Type::Double);
    smethod(st, math, "ceil", vec![Type::Double], Type::Double);
    smethod(st, math, "round", vec![Type::Double], Type::Long);
    smethod(st, math, "round", vec![Type::Float], Type::Int);
    smethod(st, math, "random", vec![], Type::Double);
    smethod(st, math, "exp", vec![Type::Double], Type::Double);
    smethod(st, math, "log", vec![Type::Double], Type::Double);
}

/// Gaps in `RichInt`/`RichLong`/`RichDouble`/`RichChar`/`RichByte`/
/// `RichShort`/`RichBoolean`.
fn add_numeric_enrichment_extra(st: &mut SymbolTable) {
    let rich_int = find(st, st.scala_pkg, "RichInt", SymKind::Class);
    if !rich_int.is_none() {
        method(
            st,
            rich_int,
            "toBinaryString",
            vec![],
            Type::String,
            Intrinsic::None,
        );
        method(
            st,
            rich_int,
            "toHexString",
            vec![],
            Type::String,
            Intrinsic::None,
        );
        method(
            st,
            rich_int,
            "toOctalString",
            vec![],
            Type::String,
            Intrinsic::None,
        );
        method(st, rich_int, "sign", vec![], Type::Int, Intrinsic::None);
    }
    let rich_long = find(st, st.scala_pkg, "RichLong", SymKind::Class);
    if !rich_long.is_none() {
        method(
            st,
            rich_long,
            "toBinaryString",
            vec![],
            Type::String,
            Intrinsic::None,
        );
        method(
            st,
            rich_long,
            "toHexString",
            vec![],
            Type::String,
            Intrinsic::None,
        );
        method(st, rich_long, "sign", vec![], Type::Long, Intrinsic::None);
    }
    let rich_double = find(st, st.scala_pkg, "RichDouble", SymKind::Class);
    if !rich_double.is_none() {
        method(
            st,
            rich_double,
            "isNaN",
            vec![],
            Type::Boolean,
            Intrinsic::None,
        );
        method(
            st,
            rich_double,
            "isInfinity",
            vec![],
            Type::Boolean,
            Intrinsic::None,
        );
        method(
            st,
            rich_double,
            "round",
            vec![],
            Type::Long,
            Intrinsic::None,
        );
        method(
            st,
            rich_double,
            "floor",
            vec![],
            Type::Double,
            Intrinsic::None,
        );
        method(
            st,
            rich_double,
            "ceil",
            vec![],
            Type::Double,
            Intrinsic::None,
        );
        method(
            st,
            rich_double,
            "sign",
            vec![],
            Type::Double,
            Intrinsic::None,
        );
    }
    let rich_char = find(st, st.scala_pkg, "RichChar", SymKind::Class);
    if !rich_char.is_none() {
        method(
            st,
            rich_char,
            "isLetter",
            vec![],
            Type::Boolean,
            Intrinsic::None,
        );
        method(
            st,
            rich_char,
            "isLetterOrDigit",
            vec![],
            Type::Boolean,
            Intrinsic::None,
        );
        method(
            st,
            rich_char,
            "isUpper",
            vec![],
            Type::Boolean,
            Intrinsic::None,
        );
        method(
            st,
            rich_char,
            "isLower",
            vec![],
            Type::Boolean,
            Intrinsic::None,
        );
        method(
            st,
            rich_char,
            "isWhitespace",
            vec![],
            Type::Boolean,
            Intrinsic::None,
        );
        method(
            st,
            rich_char,
            "toUpper",
            vec![],
            Type::Char,
            Intrinsic::None,
        );
        method(
            st,
            rich_char,
            "toLower",
            vec![],
            Type::Char,
            Intrinsic::None,
        );
    }
    // `RichByte`/`RichShort.compare` are skipped: like the other numeric
    // `compare`s they have no `$extension` static counterpart, and (unlike
    // `RichBoolean.compare`, whose real-instance-allocation codegen already
    // existed in gen.rs) wiring them up would need new codegen support.
    let rich_bool = find(st, st.scala_pkg, "RichBoolean", SymKind::Class);
    if !rich_bool.is_none() {
        method(
            st,
            rich_bool,
            "compare",
            vec![Type::Boolean],
            Type::Int,
            Intrinsic::None,
        );
    }
}
