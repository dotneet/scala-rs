use crate::prelude::{class, fn1, fn2, method, module, type_param};
use crate::symbol::{Intrinsic, SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

pub(crate) fn add_string_ops(st: &mut SymbolTable, iterator: SymbolId) -> SymbolId {
    let so = class(
        st,
        st.scala_pkg,
        "StringOps",
        "scala/collection/StringOps",
        &[Type::AnyVal],
    );
    let f = st.alloc("repr", so, SymKind::Term, Flags::PARAM, "");
    st.get_mut(f).ty = Type::String;
    st.get_mut(so).ctor_fields = vec![f];
    method(st, so, "toInt", vec![], Type::Int, Intrinsic::None);
    method(st, so, "toLong", vec![], Type::Long, Intrinsic::None);
    method(st, so, "toDouble", vec![], Type::Double, Intrinsic::None);
    method(st, so, "length", vec![], Type::Int, Intrinsic::None);
    method(st, so, "size", vec![], Type::Int, Intrinsic::None);
    method(st, so, "isEmpty", vec![], Type::Boolean, Intrinsic::None);
    method(st, so, "*", vec![Type::Int], Type::String, Intrinsic::None);
    method(
        st,
        so,
        "take",
        vec![Type::Int],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "drop",
        vec![Type::Int],
        Type::String,
        Intrinsic::None,
    );
    method(st, so, "toUpperCase", vec![], Type::String, Intrinsic::None);
    method(st, so, "toLowerCase", vec![], Type::String, Intrinsic::None);
    method(
        st,
        so,
        "stripPrefix",
        vec![Type::String],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "split",
        vec![Type::Char],
        Type::Array(Box::new(Type::String)),
        Intrinsic::None,
    );
    method(
        st,
        so,
        "stripSuffix",
        vec![Type::String],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "padTo",
        vec![Type::Int, Type::Char],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "linesIterator",
        vec![],
        Type::Class {
            sym: iterator,
            args: vec![Type::String],
        },
        Intrinsic::None,
    );
    method(
        st,
        so,
        "toIntOption",
        vec![],
        Type::Class {
            sym: st.option_sym,
            args: vec![Type::Int],
        },
        Intrinsic::None,
    );
    method(st, so, "stripMargin", vec![], Type::String, Intrinsic::None);
    method(
        st,
        so,
        "stripMargin",
        vec![Type::Char],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "lines",
        vec![],
        Type::Class {
            sym: iterator,
            args: vec![Type::String],
        },
        Intrinsic::None,
    );
    method(st, so, "capitalize", vec![], Type::String, Intrinsic::None);
    method(st, so, "reverse", vec![], Type::String, Intrinsic::None);
    method(
        st,
        so,
        "slice",
        vec![Type::Int, Type::Int],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "takeRight",
        vec![Type::Int],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "dropRight",
        vec![Type::Int],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "contains",
        vec![Type::Char],
        Type::Boolean,
        Intrinsic::None,
    );
    method(st, so, "head", vec![], Type::Char, Intrinsic::None);
    method(st, so, "last", vec![], Type::Char, Intrinsic::None);
    method(
        st,
        so,
        "stripLineEnd",
        vec![],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "replaceAllLiterally",
        vec![Type::String, Type::String],
        Type::String,
        Intrinsic::None,
    );
    method(st, so, "tail", vec![], Type::String, Intrinsic::None);
    method(st, so, "init", vec![], Type::String, Intrinsic::None);
    method(st, so, "distinct", vec![], Type::String, Intrinsic::None);
    method(st, so, "mkString", vec![], Type::String, Intrinsic::None);
    method(
        st,
        so,
        "mkString",
        vec![Type::String],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "mkString",
        vec![Type::String, Type::String, Type::String],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "filter",
        vec![fn1(Type::Char, Type::Boolean)],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "reverseIterator",
        vec![],
        Type::Class {
            sym: iterator,
            args: vec![Type::Char],
        },
        Intrinsic::None,
    );
    let seq = crate::classpath::find_or_stub_java_class(st, "scala/collection/Seq");
    // `Seq` carries a type parameter (`prelude_hier` gives the stub one), so
    // name the element type instead of leaving the parameter raw.
    let seq_char = Type::Class {
        sym: seq,
        args: vec![Type::Char],
    };
    method(
        st,
        so,
        "diff",
        vec![seq_char],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "intersect",
        vec![Type::Class {
            sym: seq,
            args: vec![],
        }],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "updated",
        vec![Type::Int, Type::Char],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "count",
        vec![fn1(Type::Char, Type::Boolean)],
        Type::Int,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "exists",
        vec![fn1(Type::Char, Type::Boolean)],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "forall",
        vec![fn1(Type::Char, Type::Boolean)],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "copyToArray",
        vec![Type::Array(Box::new(Type::Char))],
        Type::Int,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "copyToArray",
        vec![Type::Array(Box::new(Type::Char)), Type::Int],
        Type::Int,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "copyToArray",
        vec![Type::Array(Box::new(Type::Char)), Type::Int, Type::Int],
        Type::Int,
        Intrinsic::None,
    );
    method(st, so, "nonEmpty", vec![], Type::Boolean, Intrinsic::None);
    method(
        st,
        so,
        "takeWhile",
        vec![fn1(Type::Char, Type::Boolean)],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "dropWhile",
        vec![fn1(Type::Char, Type::Boolean)],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "filterNot",
        vec![fn1(Type::Char, Type::Boolean)],
        Type::String,
        Intrinsic::None,
    );
    let opt_char = Type::Class {
        sym: st.option_sym,
        args: vec![Type::Char],
    };
    method(
        st,
        so,
        "headOption",
        vec![],
        opt_char.clone(),
        Intrinsic::None,
    );
    method(
        st,
        so,
        "lastOption",
        vec![],
        opt_char.clone(),
        Intrinsic::None,
    );
    method(
        st,
        so,
        "find",
        vec![fn1(Type::Char, Type::Boolean)],
        opt_char,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "foreach",
        vec![fn1(Type::Char, Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    method(st, so, "toBoolean", vec![], Type::Boolean, Intrinsic::None);
    method(
        st,
        so,
        "toBooleanOption",
        vec![],
        Type::Class {
            sym: st.option_sym,
            args: vec![Type::Boolean],
        },
        Intrinsic::None,
    );
    method(st, so, "toByte", vec![], Type::Byte, Intrinsic::None);
    method(st, so, "toShort", vec![], Type::Short, Intrinsic::None);
    method(st, so, "toFloat", vec![], Type::Float, Intrinsic::None);
    method(
        st,
        so,
        "toByteOption",
        vec![],
        Type::Class {
            sym: st.option_sym,
            args: vec![Type::Byte],
        },
        Intrinsic::None,
    );
    method(
        st,
        so,
        "toShortOption",
        vec![],
        Type::Class {
            sym: st.option_sym,
            args: vec![Type::Short],
        },
        Intrinsic::None,
    );
    method(
        st,
        so,
        "toFloatOption",
        vec![],
        Type::Class {
            sym: st.option_sym,
            args: vec![Type::Float],
        },
        Intrinsic::None,
    );
    method(
        st,
        so,
        "toLongOption",
        vec![],
        Type::Class {
            sym: st.option_sym,
            args: vec![Type::Long],
        },
        Intrinsic::None,
    );
    method(
        st,
        so,
        "toDoubleOption",
        vec![],
        Type::Class {
            sym: st.option_sym,
            args: vec![Type::Double],
        },
        Intrinsic::None,
    );
    so
}
/// StringOps.foldLeft[B](z: B)(op: (B, Char) => B): B
///
/// nsc 2.13.16 JVM: `foldLeft$extension(String, Object, Function2)Object`.
pub(crate) fn add_string_ops_fold_left(st: &mut SymbolTable, so: SymbolId) {
    let m = method(st, so, "foldLeft", vec![], Type::Unit, Intrinsic::None);
    let b = type_param(st, m, "B");
    let tb = Type::TypeParam(b);
    let z = st.alloc("z", m, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(z).ty = tb.clone();
    let op = st.alloc("op", m, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(op).ty = fn2(tb.clone(), Type::Char, tb.clone());
    st.get_mut(m).tparams = vec![b];
    st.get_mut(m).params = vec![z, op];
    st.get_mut(m).paramss = vec![vec![z], vec![op]];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![
            vec![tb.clone()],
            vec![fn2(tb.clone(), Type::Char, tb.clone())],
        ],
        ret: Box::new(tb),
    };
}
/// StringOps.foldRight[B](z: B)(op: (Char, B) => B): B and grouped(n): Iterator[String].
///
/// nsc 2.13.16 JVM: `foldRight$extension(String, Object, Function2)Object` /
/// `grouped$extension(String, I)Iterator`.
pub(crate) fn add_string_ops_fold_right_and_grouped(st: &mut SymbolTable, so: SymbolId) {
    let m = method(st, so, "foldRight", vec![], Type::Unit, Intrinsic::None);
    let b = type_param(st, m, "B");
    let tb = Type::TypeParam(b);
    let z = st.alloc("z", m, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(z).ty = tb.clone();
    let op = st.alloc("op", m, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(op).ty = fn2(Type::Char, tb.clone(), tb.clone());
    st.get_mut(m).tparams = vec![b];
    st.get_mut(m).params = vec![z, op];
    st.get_mut(m).paramss = vec![vec![z], vec![op]];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![
            vec![tb.clone()],
            vec![fn2(Type::Char, tb.clone(), tb.clone())],
        ],
        ret: Box::new(tb),
    };
    let it = st
        .lookup_member(st.scala_pkg, "Iterator")
        .into_iter()
        .find(|&id| {
            matches!(
                st.get(id).kind,
                crate::symbol::SymKind::Class | crate::symbol::SymKind::ModuleClass
            )
        })
        .unwrap_or(SymbolId::NONE);
    method(
        st,
        so,
        "grouped",
        vec![Type::Int],
        Type::Class {
            sym: it,
            args: vec![Type::String],
        },
        Intrinsic::None,
    );
}
/// StringOps.map(Char => Char): String, `:+` / `+:` against 2.13.16.
///
/// JVM: `map$extension(String, Function1)String`,
/// `$colon$plus$extension(String, C)String`, `$plus$colon$extension(String, C)String`.
pub(crate) fn add_string_ops_map_and_appended(st: &mut SymbolTable, so: SymbolId) {
    method(
        st,
        so,
        "map",
        vec![fn1(Type::Char, Type::Char)],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        so,
        ":+",
        vec![Type::Char],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "+:",
        vec![Type::Char],
        Type::String,
        Intrinsic::None,
    );
}
/// StringOps.compare / lengthCompare / patch(Int, String, Int) / `<` / `>` /
/// `>=` / `<=` against 2.13.16.
///
/// JVM: `compare$extension(String, String)I`, `lengthCompare$extension(String, I)I`,
/// `patch$extension(String, I, String, I)String`, `$less$extension` /
/// `$greater$extension` / `$greater$eq$extension` / `$less$eq$extension`
/// `(String, String)Z`.
pub(crate) fn add_string_ops_compare_patch_length(st: &mut SymbolTable, so: SymbolId) {
    method(
        st,
        so,
        "compare",
        vec![Type::String],
        Type::Int,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "lengthCompare",
        vec![Type::Int],
        Type::Int,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "patch",
        vec![Type::Int, Type::String, Type::Int],
        Type::String,
        Intrinsic::None,
    );
    for op in ["<", ">", ">=", "<="] {
        method(
            st,
            so,
            op,
            vec![Type::String],
            Type::Boolean,
            Intrinsic::None,
        );
    }
}
/// StringOps.iterator / sizeCompare / knownSize / appendedAll / prependedAll
/// against 2.13.16.
///
/// JVM: `iterator$extension(String)Iterator`, `sizeCompare$extension(String, I)I`,
/// `knownSize$extension(String)I`, `appendedAll$extension` /
/// `prependedAll$extension(String, String)String`.
pub(crate) fn add_string_ops_iterator_size_appended(
    st: &mut SymbolTable,
    so: SymbolId,
    iterator: SymbolId,
) {
    method(
        st,
        so,
        "iterator",
        vec![],
        Type::Class {
            sym: iterator,
            args: vec![Type::Char],
        },
        Intrinsic::None,
    );
    method(
        st,
        so,
        "sizeCompare",
        vec![Type::Int],
        Type::Int,
        Intrinsic::None,
    );
    method(st, so, "knownSize", vec![], Type::Int, Intrinsic::None);
    method(
        st,
        so,
        "appendedAll",
        vec![Type::String],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "prependedAll",
        vec![Type::String],
        Type::String,
        Intrinsic::None,
    );
}
/// StringOps.`++` / lengthIs / sizeIs / flatMap(Char => String) against 2.13.16.
///
/// JVM: `$plus$plus$extension(String, String)String`,
/// `lengthIs$extension` / `sizeIs$extension(String)I`,
/// `flatMap$extension(String, Function1)String`.
pub(crate) fn add_string_ops_concat_length_flat(st: &mut SymbolTable, so: SymbolId) {
    method(
        st,
        so,
        "++",
        vec![Type::String],
        Type::String,
        Intrinsic::None,
    );
    method(st, so, "lengthIs", vec![], Type::Int, Intrinsic::None);
    method(st, so, "sizeIs", vec![], Type::Int, Intrinsic::None);
    method(
        st,
        so,
        "flatMap",
        vec![fn1(Type::Char, Type::String)],
        Type::String,
        Intrinsic::None,
    );
}
pub(crate) fn add_string_ops_indices_and_r(st: &mut SymbolTable, so: SymbolId) {
    let range = st
        .lookup_member(st.scala_pkg, "Range")
        .into_iter()
        .find(|&id| st.get(id).kind == crate::symbol::SymKind::Class)
        .unwrap_or(SymbolId::NONE);
    method(
        st,
        so,
        "indices",
        vec![],
        Type::Class {
            sym: range,
            args: vec![],
        },
        Intrinsic::None,
    );
    let matching = crate::classpath::ensure_package(st, "scala/util/matching");
    let regex = class(
        st,
        matching,
        "Regex",
        "scala/util/matching/Regex",
        &[Type::AnyRef],
    );
    method(
        st,
        regex,
        "findFirstIn",
        vec![Type::String],
        Type::Class {
            sym: st.option_sym,
            args: vec![Type::String],
        },
        Intrinsic::None,
    );
    method(
        st,
        regex,
        "matches",
        vec![Type::String],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "r",
        vec![],
        Type::Class {
            sym: regex,
            args: vec![],
        },
        Intrinsic::None,
    );
}
/// `StringOps.toArray` with `ClassTag[Char]` — nsc `toArray[B >: Char : ClassTag]`.
pub(crate) fn add_string_ops_to_array(st: &mut SymbolTable, so: SymbolId, ct: SymbolId) {
    let m = method(
        st,
        so,
        "toArray",
        vec![],
        Type::Array(Box::new(Type::Char)),
        Intrinsic::None,
    );
    let ev = st.alloc(
        "evidence$1",
        m,
        crate::symbol::SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(ev).ty = Type::Class {
        sym: ct,
        args: vec![Type::Char],
    };
    st.get_mut(m).params = vec![ev];
    st.get_mut(m).paramss = vec![vec![ev]];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![vec![Type::Class {
            sym: ct,
            args: vec![Type::Char],
        }]],
        ret: Box::new(Type::Array(Box::new(Type::Char))),
    };
}
/// `StringOps.sorted` with implicit `Ordering[Char]` (`Ordering$Char$.MODULE$`).
pub(crate) fn add_string_ops_sorted(st: &mut SymbolTable, so: SymbolId, ordering: SymbolId) {
    let m = method(st, so, "sorted", vec![], Type::String, Intrinsic::None);
    let ev = st.alloc(
        "evidence$1",
        m,
        crate::symbol::SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(ev).ty = Type::Class {
        sym: ordering,
        args: vec![Type::Char],
    };
    st.get_mut(m).params = vec![ev];
    st.get_mut(m).paramss = vec![vec![ev]];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![vec![Type::Class {
            sym: ordering,
            args: vec![Type::Char],
        }]],
        ret: Box::new(Type::String),
    };
}
/// `StringContext.parts` is a `Seq[String]`; `Seq` only exists once
/// `add_seq_and_lazylist` has run, so the type is filled in afterwards.
pub(crate) fn fix_string_context_parts(st: &mut SymbolTable) {
    let Some(seq) = crate::classpath::find_by_jvm(st, "scala/collection/immutable/Seq") else {
        return;
    };
    let Some(sc) = crate::classpath::find_by_jvm(st, "scala/StringContext") else {
        return;
    };
    let fields = st.get(sc).ctor_fields.clone();
    for f in fields {
        if st.get(f).name == "parts" {
            st.get_mut(f).ty = Type::Class {
                sym: seq,
                args: vec![Type::String],
            };
        }
    }
}
pub(crate) fn add_string_context(st: &mut SymbolTable) {
    let sc = class(
        st,
        st.scala_pkg,
        "StringContext",
        "scala/StringContext",
        &[Type::AnyRef],
    );
    // `new StringContext(parts: String*)` takes a repeated parameter, but the
    // member `parts` is a `Seq[String]`.
    let parts = st.alloc("parts", sc, SymKind::Term, Flags::PARAM, "parts");
    let seq = crate::classpath::find_by_jvm(st, "scala/collection/immutable/Seq");
    st.get_mut(parts).ty = match seq {
        Some(seq) => Type::Class {
            sym: seq,
            args: vec![Type::String],
        },
        None => Type::Repeated(Box::new(Type::String)),
    };
    st.get_mut(sc).ctor_fields = vec![parts];
    method(
        st,
        sc,
        "s",
        vec![Type::Repeated(Box::new(Type::Any))],
        Type::String,
        Intrinsic::None,
    );
    let scm = module(st, st.scala_pkg, "StringContext", "scala/StringContext$");
    let mc = st.module_class_of(scm);
    method(
        st,
        mc,
        "apply",
        vec![Type::Repeated(Box::new(Type::String))],
        Type::Class {
            sym: sc,
            args: vec![],
        },
        Intrinsic::None,
    );
    let mems = st.get(mc).members.clone();
    st.get_mut(scm).members.extend(mems);
}
