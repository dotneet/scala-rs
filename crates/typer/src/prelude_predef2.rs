use crate::prelude::{class, method, type_param};
use crate::symbol::{Intrinsic, SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

pub(crate) fn add_predef_members(
    st: &mut SymbolTable,
    arrow: SymbolId,
    string_ops: Option<SymbolId>,
    array_ops: Option<SymbolId>,
    rich_int: Option<SymbolId>,
    rich_ldc: Option<(SymbolId, SymbolId, SymbolId)>,
    library_abi: bool,
) {
    let p = st.predef;
    let cls = st.get(p).ty.clone();
    let owner = match cls {
        Type::ModuleRef(id) => id,
        _ => p,
    };
    // nsc `Predef.classOf[T]: Class[T]` — a class literal, not a real call.
    if let Some(jclass) = crate::classpath::find_by_jvm(st, "java/lang/Class") {
        let co = method(st, owner, "classOf", vec![], Type::Any, Intrinsic::ClassOf);
        let t = type_param(st, co, "T");
        st.get_mut(co).tparams = vec![t];
        st.get_mut(co).ty = Type::Method {
            paramss: Vec::new(),
            ret: Box::new(Type::Class {
                sym: jclass,
                args: vec![Type::TypeParam(t)],
            }),
        };
    }
    // `Any.getClass(): Class[_]`, inherited from `java.lang.Object`.
    if let Some(jclass) = crate::classpath::find_by_jvm(st, "java/lang/Class") {
        let any = st.any_sym;
        method(
            st,
            any,
            "getClass",
            vec![],
            Type::Class {
                sym: jclass,
                args: vec![Type::Any],
            },
            Intrinsic::GetClass,
        );
    }
    method(st, owner, "println", vec![], Type::Unit, Intrinsic::Println);
    method(
        st,
        owner,
        "println",
        vec![Type::Int],
        Type::Unit,
        Intrinsic::Println,
    );
    method(
        st,
        owner,
        "println",
        vec![Type::Long],
        Type::Unit,
        Intrinsic::Println,
    );
    method(
        st,
        owner,
        "println",
        vec![Type::Double],
        Type::Unit,
        Intrinsic::Println,
    );
    method(
        st,
        owner,
        "println",
        vec![Type::Boolean],
        Type::Unit,
        Intrinsic::Println,
    );
    method(
        st,
        owner,
        "println",
        vec![Type::String],
        Type::Unit,
        Intrinsic::Println,
    );
    method(
        st,
        owner,
        "println",
        vec![Type::Any],
        Type::Unit,
        Intrinsic::Println,
    );
    method(
        st,
        owner,
        "print",
        vec![Type::Any],
        Type::Unit,
        Intrinsic::Print,
    );
    method(
        st,
        owner,
        "assert",
        vec![Type::Boolean],
        Type::Unit,
        Intrinsic::Assert,
    );
    method(
        st,
        owner,
        "assert",
        vec![Type::Boolean, Type::ByName(Box::new(Type::Any))],
        Type::Unit,
        Intrinsic::Assert,
    );
    method(
        st,
        owner,
        "require",
        vec![Type::Boolean],
        Type::Unit,
        Intrinsic::Require,
    );
    method(
        st,
        owner,
        "require",
        vec![Type::Boolean, Type::ByName(Box::new(Type::Any))],
        Type::Unit,
        Intrinsic::Require,
    );
    method(
        st,
        owner,
        "???",
        vec![],
        Type::Nothing,
        Intrinsic::NotImplemented,
    );
    let ident = method(
        st,
        owner,
        "identity",
        vec![Type::Any],
        Type::Any,
        Intrinsic::Identity,
    );
    let ia = type_param(st, ident, "A");
    st.get_mut(ident).tparams = vec![ia];
    st.get_mut(ident).ty = Type::Method {
        paramss: vec![vec![Type::TypeParam(ia)]],
        ret: Box::new(Type::TypeParam(ia)),
    };
    let loc = method(
        st,
        owner,
        "locally",
        vec![Type::Any],
        Type::Any,
        Intrinsic::Locally,
    );
    let lt = type_param(st, loc, "A");
    st.get_mut(loc).tparams = vec![lt];
    st.get_mut(loc).ty = Type::Method {
        paramss: vec![vec![Type::TypeParam(lt)]],
        ret: Box::new(Type::TypeParam(lt)),
    };
    let implm = method(
        st,
        owner,
        "implicitly",
        vec![Type::Any],
        Type::Any,
        Intrinsic::Implicitly,
    );
    let it = type_param(st, implm, "T");
    let ip = st.alloc(
        "e",
        implm,
        crate::symbol::SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(ip).ty = Type::TypeParam(it);
    st.get_mut(implm).tparams = vec![it];
    st.get_mut(implm).params = vec![ip];
    st.get_mut(implm).paramss = vec![vec![ip]];
    st.get_mut(implm).ty = Type::Method {
        paramss: vec![vec![Type::TypeParam(it)]],
        ret: Box::new(Type::TypeParam(it)),
    };
    let sadd = if library_abi {
        let s = class(
            st,
            st.scala_pkg,
            "any2stringadd",
            "scala/Predef$any2stringadd",
            &[Type::AnyVal],
        );
        let f = st.alloc("self", s, SymKind::Term, Flags::PARAM, "");
        st.get_mut(f).ty = Type::Any;
        st.get_mut(s).ctor_fields = vec![f];
        method(
            st,
            s,
            "+",
            vec![Type::String],
            Type::String,
            Intrinsic::None,
        );
        s
    } else {
        let s = class(
            st,
            st.scala_pkg,
            "any2stringadd",
            "scala/runtime/StringAdd",
            &[Type::AnyRef],
        );
        method(
            st,
            s,
            "+",
            vec![Type::String],
            Type::String,
            Intrinsic::StringConcat,
        );
        s
    };
    let conv_s = method(
        st,
        owner,
        "any2stringadd",
        vec![Type::Any],
        Type::Class {
            sym: sadd,
            args: vec![],
        },
        if library_abi {
            Intrinsic::Identity
        } else {
            Intrinsic::Any2StringAdd
        },
    );
    st.get_mut(conv_s).flags = st.get(conv_s).flags.with(Flags::IMPLICIT);
    let conv = method(
        st,
        owner,
        "any2ArrowAssoc",
        vec![Type::Any],
        Type::Class {
            sym: arrow,
            args: vec![],
        },
        Intrinsic::WrapArrowAssoc,
    );
    st.get_mut(conv).flags = st.get(conv).flags.with(Flags::IMPLICIT);
    if let Some(sops) = string_ops {
        let aug = method(
            st,
            owner,
            "augmentString",
            vec![Type::String],
            Type::Class {
                sym: sops,
                args: vec![],
            },
            Intrinsic::Identity,
        );
        st.get_mut(aug).flags = st.get(aug).flags.with(Flags::IMPLICIT);
        let seq = crate::classpath::find_or_stub_java_class(st, "scala/collection/Seq");
        let ws = class(
            st,
            st.scala_pkg,
            "WrappedString",
            "scala/collection/immutable/WrappedString",
            &[Type::Class {
                sym: seq,
                args: vec![Type::Char],
            }],
        );
        let wrap_str = method(
            st,
            owner,
            "wrapString",
            vec![Type::String],
            Type::Class {
                sym: ws,
                args: vec![],
            },
            Intrinsic::None,
        );
        st.get_mut(wrap_str).flags = st.get(wrap_str).flags.with(Flags::IMPLICIT);
        // javap: `wrapString` is declared on `scala.LowPriorityImplicits`,
        // `augmentString` on `Predef$`. So `StringOps` outranks
        // `WrappedString` whenever both offer the selected member -- which is
        // what `search_extension` already documents but could not act on
        // while this flag was unset (only `intWrapper` & co. carried it).
        st.get_mut(wrap_str).low_priority = true;
    }
    if let Some(aops) = array_ops {
        let wrap = method(
            st,
            owner,
            "intArrayOps",
            vec![Type::Array(Box::new(Type::Int))],
            Type::Class {
                sym: aops,
                args: vec![Type::Int],
            },
            Intrinsic::Identity,
        );
        st.get_mut(wrap).flags = st.get(wrap).flags.with(Flags::IMPLICIT);
        let wrap_l = method(
            st,
            owner,
            "longArrayOps",
            vec![Type::Array(Box::new(Type::Long))],
            Type::Class {
                sym: aops,
                args: vec![Type::Long],
            },
            Intrinsic::Identity,
        );
        st.get_mut(wrap_l).flags = st.get(wrap_l).flags.with(Flags::IMPLICIT);
        let wrap_b = method(
            st,
            owner,
            "byteArrayOps",
            vec![Type::Array(Box::new(Type::Byte))],
            Type::Class {
                sym: aops,
                args: vec![Type::Byte],
            },
            Intrinsic::Identity,
        );
        st.get_mut(wrap_b).flags = st.get(wrap_b).flags.with(Flags::IMPLICIT);
        let wrap_s = method(
            st,
            owner,
            "shortArrayOps",
            vec![Type::Array(Box::new(Type::Short))],
            Type::Class {
                sym: aops,
                args: vec![Type::Short],
            },
            Intrinsic::Identity,
        );
        st.get_mut(wrap_s).flags = st.get(wrap_s).flags.with(Flags::IMPLICIT);
        let wrap_c = method(
            st,
            owner,
            "charArrayOps",
            vec![Type::Array(Box::new(Type::Char))],
            Type::Class {
                sym: aops,
                args: vec![Type::Char],
            },
            Intrinsic::Identity,
        );
        st.get_mut(wrap_c).flags = st.get(wrap_c).flags.with(Flags::IMPLICIT);
        let wrap_f = method(
            st,
            owner,
            "floatArrayOps",
            vec![Type::Array(Box::new(Type::Float))],
            Type::Class {
                sym: aops,
                args: vec![Type::Float],
            },
            Intrinsic::Identity,
        );
        st.get_mut(wrap_f).flags = st.get(wrap_f).flags.with(Flags::IMPLICIT);
        let wrap_d = method(
            st,
            owner,
            "doubleArrayOps",
            vec![Type::Array(Box::new(Type::Double))],
            Type::Class {
                sym: aops,
                args: vec![Type::Double],
            },
            Intrinsic::Identity,
        );
        st.get_mut(wrap_d).flags = st.get(wrap_d).flags.with(Flags::IMPLICIT);
        let wrap_bool = method(
            st,
            owner,
            "booleanArrayOps",
            vec![Type::Array(Box::new(Type::Boolean))],
            Type::Class {
                sym: aops,
                args: vec![Type::Boolean],
            },
            Intrinsic::Identity,
        );
        st.get_mut(wrap_bool).flags = st.get(wrap_bool).flags.with(Flags::IMPLICIT);
        let wrap_u = method(
            st,
            owner,
            "unitArrayOps",
            vec![Type::Array(Box::new(Type::Unit))],
            Type::Class {
                sym: aops,
                args: vec![Type::Unit],
            },
            Intrinsic::Identity,
        );
        st.get_mut(wrap_u).flags = st.get(wrap_u).flags.with(Flags::IMPLICIT);
        let wrap_ref = method(
            st,
            owner,
            "refArrayOps",
            vec![Type::Array(Box::new(Type::AnyRef))],
            Type::Class {
                sym: aops,
                args: vec![Type::AnyRef],
            },
            Intrinsic::Identity,
        );
        let rt = type_param(st, wrap_ref, "T");
        st.get_mut(wrap_ref).tparams = vec![rt];
        st.get_mut(wrap_ref).ty = Type::Method {
            paramss: vec![vec![Type::Array(Box::new(Type::TypeParam(rt)))]],
            ret: Box::new(Type::Class {
                sym: aops,
                args: vec![Type::TypeParam(rt)],
            }),
        };
        st.get_mut(wrap_ref).flags = st.get(wrap_ref).flags.with(Flags::IMPLICIT);
        // nsc Predef.genericArrayOps[T](xs: Array[T]): ArrayOps[T] — the only
        // conversion that applies to an unconstrained type parameter `Array[T]`
        // (refArrayOps requires T <: AnyRef; primitive wrappers need Array[Int] etc.).
        let wrap_g = method(
            st,
            owner,
            "genericArrayOps",
            vec![Type::Array(Box::new(Type::Any))],
            Type::Class {
                sym: aops,
                args: vec![Type::Any],
            },
            Intrinsic::Identity,
        );
        let gt = type_param(st, wrap_g, "T");
        st.get_mut(wrap_g).tparams = vec![gt];
        st.get_mut(wrap_g).ty = Type::Method {
            paramss: vec![vec![Type::Array(Box::new(Type::TypeParam(gt)))]],
            ret: Box::new(Type::Class {
                sym: aops,
                args: vec![Type::TypeParam(gt)],
            }),
        };
        st.get_mut(wrap_g).flags = st.get(wrap_g).flags.with(Flags::IMPLICIT);
    }
    if let Some(ri) = rich_int {
        let wrap = method(
            st,
            owner,
            "intWrapper",
            vec![Type::Int],
            Type::Class {
                sym: ri,
                args: vec![],
            },
            Intrinsic::Identity,
        );
        st.get_mut(wrap).flags = st.get(wrap).flags.with(Flags::IMPLICIT);
    }
    if let Some((rl, rd, rc)) = rich_ldc {
        add_numeric_wrapper(st, owner, "longWrapper", Type::Long, rl);
        add_numeric_wrapper(st, owner, "doubleWrapper", Type::Double, rd);
        add_numeric_wrapper(st, owner, "charWrapper", Type::Char, rc);
    }
    if library_abi {
        let rf = crate::prelude_richnum::add_rich_float(st);
        add_numeric_wrapper(st, owner, "floatWrapper", Type::Float, rf);
        let (rb, rs, rbool) = crate::prelude_richnum::add_rich_byte_short_boolean(st);
        add_numeric_wrapper(st, owner, "byteWrapper", Type::Byte, rb);
        add_numeric_wrapper(st, owner, "shortWrapper", Type::Short, rs);
        add_numeric_wrapper(st, owner, "booleanWrapper", Type::Boolean, rbool);
        let mutp = crate::classpath::ensure_package(st, "scala/collection/mutable");
        if let Some(of_int) = st
            .lookup_member(mutp, "ArraySeq$ofInt")
            .into_iter()
            .find(|&id| st.get(id).kind == crate::symbol::SymKind::Class)
        {
            // nsc LowPriorityImplicits.wrapIntArray — not IMPLICIT here so it
            // does not compete with intArrayOps for Array members.
            method(
                st,
                owner,
                "wrapIntArray",
                vec![Type::Array(Box::new(Type::Int))],
                Type::Class {
                    sym: of_int,
                    args: vec![],
                },
                Intrinsic::None,
            );
        }
    }
    let mems = st.get(owner).members.clone();
    st.get_mut(p).members.extend(mems.iter().copied());
    for m in mems {
        let name = st.get(m).name.clone();
        st.enter_in_current(&name, m);
    }
}
fn add_numeric_wrapper(
    st: &mut SymbolTable,
    owner: SymbolId,
    name: &str,
    from: Type,
    cls: SymbolId,
) {
    let wrap = method(
        st,
        owner,
        name,
        vec![from],
        Type::Class {
            sym: cls,
            args: vec![],
        },
        Intrinsic::Identity,
    );
    st.get_mut(wrap).flags = st.get(wrap).flags.with(Flags::IMPLICIT);
    // nsc declares `intWrapper` & co. in `LowPriorityImplicits`, which `Predef`
    // extends, so `Predef`'s own `double2Double` outranks `doubleWrapper` when
    // both results offer the selected member (`0.5.isNaN`).
    st.get_mut(wrap).low_priority = true;
}
