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
        mark_case(st, t2);
        add_companion(st, t2, 2);
    }
    // The private runtime ships only `scala/Tuple2`; emitting symbols for the
    // rest would compile to classes that are not there.
    if !library_abi {
        return;
    }
    // `Tuple1` is missing for the same reason as 3 and up. Nothing in the
    // surface syntax builds one -- `(x)` is a parenthesised expression, not a
    // one-tuple -- but the *name* is writable, and cats-kernel's generated
    // instances write it (`Eq[Tuple1[A0]]`, `Order[Tuple1[A0]]`, …).
    // `tpt_to_type` turns `Tuple1[A0]` into the structural `Type::Tuple([A0])`
    // like every other arity, and without the class behind it `class_sym_of`
    // answered `None`: `value _1 is not a member of (A0)`.
    for n in std::iter::once(1).chain(3..=MAX_TUPLE) {
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
        mark_case(st, cls);
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

/// Every `TupleN` is a `case class` in scala-library
/// (`final case class Tuple2[+T1, +T2](_1: T1, _2: T2)`), and the flag is what
/// `try_rewrite_case_copy` keys on. Without it `(a, b).copy(_1 = x)` -- which
/// cats' generated `NTuple*Instances` write 22 times -- reported `value copy
/// is not a member of (Any, Any)`. Nothing else the flag reaches applies to a
/// class the prelude builds: the `apply`/`unapply`/`Product` synthesis all
/// runs off a source `ClassDef` tree, and the pattern-matching path already
/// took the constructor arm for anything with `ctor_fields`.
fn mark_case(st: &mut SymbolTable, cls: SymbolId) {
    let flags = st.get(cls).flags.with(Flags::CASE);
    st.get_mut(cls).flags = flags;
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
