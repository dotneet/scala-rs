//! `TupleN` companions. `(1, "x")` parses to `Tuple2(1, "x")`, so the tuple
//! classes need a companion `apply`; nsc lowers that to a direct allocation.

use crate::prelude::{class, module, prelude_method, type_param};
use crate::symbol::{Intrinsic, SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

/// Highest arity scala-library defines.
const MAX_TUPLE: usize = 22;

pub(crate) fn add_tuples(st: &mut SymbolTable, library_abi: bool) {
    // `Tuple2` is built with the core prelude; only its companion is missing.
    if let Some(t2) = st
        .lookup("Tuple2")
        .into_iter()
        .find(|id| st.get(*id).kind == SymKind::Class)
    {
        add_companion(st, t2, 2);
    }
    // The private runtime ships only `scala/Tuple2`; emitting symbols for the
    // rest would compile to classes that are not there.
    if !library_abi {
        return;
    }
    for n in 3..=MAX_TUPLE {
        let name = format!("Tuple{n}");
        let jvm = format!("scala/Tuple{n}");
        let cls = class(st, st.scala_pkg, &name, &jvm, &[Type::AnyRef]);
        let tps: Vec<SymbolId> = (1..=n)
            .map(|i| type_param(st, cls, &format!("T{i}")))
            .collect();
        st.get_mut(cls).tparams = tps.clone();
        // The jar keeps `Tuple3._1` and up private, so both selection and
        // destructuring go through the `_1()` accessor.
        let fields: Vec<SymbolId> = (1..=n)
            .map(|i| {
                let name = format!("_{i}");
                let f = st.alloc(&name, cls, SymKind::Term, Flags::PARAM, &name);
                st.get_mut(f).ty = Type::TypeParam(tps[i - 1]);
                prelude_method(
                    st,
                    cls,
                    &name,
                    vec![],
                    Type::TypeParam(tps[i - 1]),
                    Intrinsic::None,
                );
                f
            })
            .collect();
        st.get_mut(cls).ctor_fields = fields;
        // `class` allocates into the package; the prelude's scope import has
        // already run, so the class needs entering by hand for patterns
        // (`case (a, b, c) =>`) to find it.
        st.enter_in_current(&name, cls);
        let params: Vec<Type> = tps.iter().map(|t| Type::TypeParam(*t)).collect();
        prelude_method(
            st,
            cls,
            "<init>",
            params,
            Type::Class {
                sym: cls,
                args: tps.iter().map(|t| Type::TypeParam(*t)).collect(),
            },
            Intrinsic::None,
        );
        add_companion(st, cls, n);
    }
}

/// `object TupleN { def apply[T1, …](x1: T1, …): TupleN[T1, …] }`.
fn add_companion(st: &mut SymbolTable, cls: SymbolId, n: usize) {
    let name = format!("Tuple{n}");
    let m = module(st, st.scala_pkg, &name, &format!("scala/Tuple{n}$"));
    let mcls = st.module_class_of(m);
    let apply = st.alloc("apply", mcls, SymKind::Method, Flags::FINAL, "");
    // The companion's parameters are its own type parameters, so `Tuple2(1, "x")`
    // infers `Tuple2[Int, String]` rather than the class's own parameters.
    let tps: Vec<SymbolId> = (1..=n)
        .map(|i| type_param(st, apply, &format!("T{i}")))
        .collect();
    st.get_mut(apply).tparams = tps.clone();
    let params: Vec<Type> = tps.iter().map(|t| Type::TypeParam(*t)).collect();
    st.get_mut(apply).ty = Type::Method {
        paramss: vec![params],
        ret: Box::new(Type::Class {
            sym: cls,
            args: tps.iter().map(|t| Type::TypeParam(*t)).collect(),
        }),
    };
    st.get_mut(apply).intrinsic = Intrinsic::NewTuple(n);
    let mems = st.get(mcls).members.clone();
    st.get_mut(m).members.extend(mems);
    // The class was imported into scope before the companion existed; without
    // this the term `Tuple2` still resolves to the class.
    st.enter_in_current(&name, m);
}
