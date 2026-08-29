//! `Ordering.TupleN`.
//!
//! `m.toList.sorted` on a `Map` needs an `Ordering[(K, V)]`, which the library
//! derives from the elements' own orderings. Without it, sorting any sequence
//! of tuples reported a missing implicit.

use crate::prelude::{prelude_method, type_param};
use crate::symbol::{Intrinsic, SymbolTable};
use scala_rs_parser::{ast::Flags, SymbolId, Type};

pub fn install(st: &mut SymbolTable, library_abi: bool) {
    if !library_abi {
        return;
    }
    let (Some(ordering), Some(module)) = (
        crate::classpath::find_by_jvm(st, "scala/math/Ordering"),
        crate::classpath::find_by_jvm(st, "scala/math/Ordering$"),
    ) else {
        return;
    };
    let mcls = st.module_class_of(module);
    for n in 2..=22usize {
        let name = format!("Tuple{n}");
        if !st.lookup_member(mcls, &name).is_empty() {
            continue;
        }
        let m = prelude_method(st, mcls, &name, vec![], Type::Any, Intrinsic::None);
        st.get_mut(m).flags = st.get(m).flags.with(Flags::IMPLICIT);
        let tps: Vec<SymbolId> = (1..=n)
            .map(|i| type_param(st, m, &format!("T{i}")))
            .collect();
        st.get_mut(m).tparams = tps.clone();
        let elems: Vec<Type> = tps.iter().map(|t| Type::TypeParam(*t)).collect();
        // `Tuple2[T1, T2](implicit ord1: Ordering[T1], ord2: Ordering[T2])`.
        let implicits: Vec<Type> = elems
            .iter()
            .map(|e| Type::Class {
                sym: ordering,
                args: vec![e.clone()],
            })
            .collect();
        st.get_mut(m).ty = Type::Method {
            paramss: vec![implicits],
            ret: Box::new(Type::Class {
                sym: ordering,
                args: vec![Type::Tuple(elems)],
            }),
        };
    }
    let mems = st.get(mcls).members.clone();
    st.get_mut(module).members = mems;
}
