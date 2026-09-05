use crate::prelude::{abs_class, class, ctor_field, iface, method, module_extending};
use crate::symbol::{Intrinsic, SymKind, SymbolTable};
use scala_rs_parser::{Flags, Type};

/// scala-xml 2.3 (`Elem(String, String, MetaData, NamespaceBinding, boolean, Seq[Node])`).
pub(crate) fn add_xml(st: &mut SymbolTable) {
    let xml = st.alloc(
        "xml",
        st.scala_pkg,
        SymKind::Package,
        Flags::PACKAGE,
        "scala/xml",
    );
    let node = abs_class(st, xml, "Node", "scala/xml/Node", &[Type::AnyRef]);
    let node_t = Type::Class {
        sym: node,
        args: vec![],
    };
    let metadata = abs_class(st, xml, "MetaData", "scala/xml/MetaData", &[Type::AnyRef]);
    let nsb = abs_class(
        st,
        xml,
        "NamespaceBinding",
        "scala/xml/NamespaceBinding",
        &[Type::AnyRef],
    );
    let _null = module_extending(
        st,
        xml,
        "Null",
        "scala/xml/Null$",
        Type::Class {
            sym: metadata,
            args: vec![],
        },
    );
    let _top = module_extending(
        st,
        xml,
        "TopScope",
        "scala/xml/TopScope$",
        Type::Class {
            sym: nsb,
            args: vec![],
        },
    );
    let seq = st
        .get(st.scala_pkg)
        .members
        .iter()
        .copied()
        .find(|&m| st.get(m).name == "Seq" && st.get(m).kind == SymKind::Class)
        .expect("Seq");
    let seq_node = Type::Class {
        sym: seq,
        args: vec![node_t.clone()],
    };
    let elem = class(st, xml, "Elem", "scala/xml/Elem", &[node_t.clone()]);
    let p_prefix = ctor_field(st, elem, "prefix", Type::String);
    let p_label = ctor_field(st, elem, "label", Type::String);
    let p_attr = ctor_field(
        st,
        elem,
        "attributes",
        Type::Class {
            sym: metadata,
            args: vec![],
        },
    );
    let p_scope = ctor_field(
        st,
        elem,
        "scope",
        Type::Class {
            sym: nsb,
            args: vec![],
        },
    );
    let p_min = ctor_field(st, elem, "minimizeEmpty", Type::Boolean);
    let p_child = ctor_field(st, elem, "child", seq_node);
    st.get_mut(elem).ctor_fields = vec![p_prefix, p_label, p_attr, p_scope, p_min, p_child];
    let text = class(st, xml, "Text", "scala/xml/Text", &[node_t.clone()]);
    let td = ctor_field(st, text, "data", Type::String);
    st.get_mut(text).ctor_fields = vec![td];
    let eref = class(
        st,
        xml,
        "EntityRef",
        "scala/xml/EntityRef",
        &[node_t.clone()],
    );
    let en = ctor_field(st, eref, "entityName", Type::String);
    st.get_mut(eref).ctor_fields = vec![en];
    let comment = class(st, xml, "Comment", "scala/xml/Comment", &[node_t.clone()]);
    let ct = ctor_field(st, comment, "commentText", Type::String);
    st.get_mut(comment).ctor_fields = vec![ct];
    let pcdata = class(st, xml, "PCData", "scala/xml/PCData", &[node_t.clone()]);
    let pd = ctor_field(st, pcdata, "data", Type::String);
    st.get_mut(pcdata).ctor_fields = vec![pd];
    let pi = class(
        st,
        xml,
        "ProcInstr",
        "scala/xml/ProcInstr",
        &[node_t.clone()],
    );
    let pit = ctor_field(st, pi, "target", Type::String);
    let pip = ctor_field(st, pi, "proctext", Type::String);
    st.get_mut(pi).ctor_fields = vec![pit, pip];
    let atom = class(st, xml, "Atom", "scala/xml/Atom", &[node_t]);
    let ad = ctor_field(st, atom, "data", Type::Any);
    st.get_mut(atom).ctor_fields = vec![ad];
    let meta_t = Type::Class {
        sym: metadata,
        args: vec![],
    };
    let upa = class(
        st,
        xml,
        "UnprefixedAttribute",
        "scala/xml/UnprefixedAttribute",
        &[meta_t.clone()],
    );
    let uk = ctor_field(st, upa, "key", Type::String);
    let uv = ctor_field(st, upa, "value", Type::String);
    let un = ctor_field(st, upa, "next", meta_t.clone());
    st.get_mut(upa).ctor_fields = vec![uk, uv, un];
    let nsb_t = Type::Class {
        sym: nsb,
        args: vec![],
    };
    let np = ctor_field(st, nsb, "prefix", Type::String);
    let nu = ctor_field(st, nsb, "uri", Type::String);
    let npar = ctor_field(st, nsb, "parent", nsb_t);
    st.get_mut(nsb).ctor_fields = vec![np, nu, npar];
    let pa = class(
        st,
        xml,
        "PrefixedAttribute",
        "scala/xml/PrefixedAttribute",
        &[meta_t.clone()],
    );
    let pp = ctor_field(st, pa, "pre", Type::String);
    let pk = ctor_field(st, pa, "key", Type::String);
    let pv = ctor_field(st, pa, "value", Type::String);
    let pn = ctor_field(st, pa, "next", meta_t);
    st.get_mut(pa).ctor_fields = vec![pp, pk, pv, pn];
}
/// `scala.Enumeration` plus inner `Value` (`Color.Red.toString` / `.id` against the jar).
pub(crate) fn add_enumeration(st: &mut SymbolTable) {
    let en = abs_class(
        st,
        st.scala_pkg,
        "Enumeration",
        "scala/Enumeration",
        &[Type::AnyRef],
    );
    let val = abs_class(st, en, "Value", "scala/Enumeration$Value", &[Type::AnyRef]);
    method(st, val, "id", vec![], Type::Int, Intrinsic::None);
    let val_t = Type::Class {
        sym: val,
        args: vec![],
    };
    method(st, en, "Value", vec![], val_t, Intrinsic::None);
}
/// `scala.DelayedInit` / `scala.App` (nsc delayed constructor body).
pub(crate) fn add_delayed_init_app(st: &mut SymbolTable) {
    let di = iface(st, st.scala_pkg, "DelayedInit", "scala/DelayedInit");
    let d = st.alloc("delayedInit", di, SymKind::Method, Flags::ABSTRACT, "");
    st.get_mut(d).ty = Type::Method {
        paramss: vec![vec![Type::ByName(Box::new(Type::Unit))]],
        ret: Box::new(Type::Unit),
    };
    let p = st.alloc("x", d, SymKind::Term, Flags::PARAM.with(Flags::BYNAME), "");
    st.get_mut(p).ty = Type::ByName(Box::new(Type::Unit));
    st.get_mut(d).params = vec![p];
    st.get_mut(d).paramss = vec![vec![p]];

    let app = iface(st, st.scala_pkg, "App", "scala/App");
    st.get_mut(app).parents = vec![
        Type::Class {
            sym: di,
            args: vec![],
        },
        Type::AnyRef,
    ];
    let d2 = st.alloc("delayedInit", app, SymKind::Method, Flags::EMPTY, "");
    st.get_mut(d2).ty = Type::Method {
        paramss: vec![vec![Type::ByName(Box::new(Type::Unit))]],
        ret: Box::new(Type::Unit),
    };
    let p2 = st.alloc("x", d2, SymKind::Term, Flags::PARAM.with(Flags::BYNAME), "");
    st.get_mut(p2).ty = Type::ByName(Box::new(Type::Unit));
    st.get_mut(d2).params = vec![p2];
    st.get_mut(d2).paramss = vec![vec![p2]];

    let main = st.alloc("main", app, SymKind::Method, Flags::EMPTY, "");
    let args_ty = Type::Array(Box::new(Type::String));
    st.get_mut(main).ty = Type::Method {
        paramss: vec![vec![args_ty.clone()]],
        ret: Box::new(Type::Unit),
    };
    let ap = st.alloc("args", main, SymKind::Term, Flags::PARAM, "");
    st.get_mut(ap).ty = args_ty;
    st.get_mut(main).params = vec![ap];
    st.get_mut(main).paramss = vec![vec![ap]];
}
