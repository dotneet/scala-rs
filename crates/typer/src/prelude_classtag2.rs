use crate::prelude::{iface, method, module, type_param};
use crate::symbol::{Intrinsic, SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

/// A parameterless getter that is *not* an implicit candidate.
fn plain_getter(st: &mut SymbolTable, owner: SymbolId, name: &str, ty: Type) {
    let id = st.alloc(name, owner, SymKind::Method, Flags::EMPTY, "");
    st.get_mut(id).ty = Type::Method {
        paramss: vec![],
        ret: Box::new(ty),
    };
}
pub(crate) fn add_classtag(st: &mut SymbolTable, jclass: SymbolId) -> SymbolId {
    let reflect = st.alloc(
        "reflect",
        st.scala_pkg,
        SymKind::Package,
        Flags::PACKAGE,
        "scala/reflect",
    );
    let ct = iface(st, reflect, "ClassTag", "scala/reflect/ClassTag");
    let t = type_param(st, ct, "T");
    st.get_mut(ct).tparams = vec![t];
    let class_ty = Type::Class {
        sym: jclass,
        args: vec![],
    };
    method(
        st,
        ct,
        "runtimeClass",
        vec![],
        class_ty.clone(),
        Intrinsic::None,
    );
    method(
        st,
        ct,
        "newArray",
        vec![Type::Int],
        Type::Array(Box::new(Type::TypeParam(t))),
        Intrinsic::None,
    );
    let ctm = module(st, reflect, "ClassTag", "scala/reflect/ClassTag$");
    let mc = st.module_class_of(ctm);
    let tag = |elem: Type| Type::Class {
        sym: ct,
        args: vec![elem],
    };
    plain_getter(st, mc, "Int", tag(Type::Int));
    plain_getter(st, mc, "Long", tag(Type::Long));
    plain_getter(st, mc, "Double", tag(Type::Double));
    plain_getter(st, mc, "Float", tag(Type::Float));
    plain_getter(st, mc, "Boolean", tag(Type::Boolean));
    plain_getter(st, mc, "Byte", tag(Type::Byte));
    plain_getter(st, mc, "Short", tag(Type::Short));
    plain_getter(st, mc, "Char", tag(Type::Char));
    plain_getter(st, mc, "Unit", tag(Type::Unit));
    plain_getter(st, mc, "Any", tag(Type::Any));
    plain_getter(st, mc, "AnyRef", tag(Type::AnyRef));
    // These are ordinary getters in Scala 2.13. They are not candidates
    // from which implicit search may infer an unconstrained type argument.
    plain_getter(st, mc, "Object", tag(Type::AnyRef));
    plain_getter(st, mc, "Nothing", tag(Type::Nothing));
    plain_getter(st, mc, "Null", tag(Type::Null));
    let apply = method(
        st,
        mc,
        "apply",
        vec![class_ty.clone()],
        tag(Type::Any),
        Intrinsic::None,
    );
    let at = type_param(st, apply, "T");
    st.get_mut(apply).tparams = vec![at];
    st.get_mut(apply).ty = Type::Method {
        paramss: vec![vec![class_ty]],
        ret: Box::new(tag(Type::TypeParam(at))),
    };
    let mems = st.get(mc).members.clone();
    st.get_mut(ctm).members.extend(mems);
    ct
}
