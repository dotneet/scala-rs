//! `Coll.empty` is properly `def empty[A]: Coll[A]` (`[K, V]` for the `Map` family).
//!
//! Many of the prelude's companions declared it monomorphically as `Coll[Any]`, so
//! `val c: Vector[Int] = Vector.empty` failed with `found: Vector[Any]` (scalac
//! accepts it). Rewriting the individual declarations collides across the whole file,
//! so instead we make them polymorphic in one pass once the prelude is assembled.
//!
//! Only the `empty`s that belong to a companion's module class, take no type
//! parameters and no arguments, and have result type `Coll[Any, …]` (with as many
//! arguments as `Coll` has type parameters) are touched. The already polymorphic ones
//! such as `ArrayDeque` / `HashMap` have type parameters, so they are left alone.

use crate::prelude::type_param;
use crate::symbol::{SymKind, SymbolTable};
use scala_rs_parser::{SymbolId, Type};

pub(crate) fn install(st: &mut SymbolTable) {
    let mut work: Vec<(SymbolId, SymbolId, usize)> = Vec::new();
    for i in 0..st.symbols.len() {
        let id = SymbolId(i as u32);
        if st.get(id).kind != SymKind::Method || st.get(id).name != "empty" {
            continue;
        }
        let s = st.get(id);
        if !s.tparams.is_empty() || !s.params.is_empty() {
            continue;
        }
        // Both `def empty: Coll[Any]` and `def empty(): Coll[Any]` qualify.
        let ret = match &s.ty {
            Type::Method { paramss, ret } if paramss.iter().all(|c| c.is_empty()) => {
                (**ret).clone()
            }
            Type::Class { .. } => s.ty.clone(),
            _ => continue,
        };
        let Type::Class { sym: coll, args } = &ret else {
            continue;
        };
        if args.is_empty() || !args.iter().all(|a| matches!(a, Type::Any)) {
            continue;
        }
        if st.get(*coll).tparams.len() != args.len() {
            continue;
        }
        // The owner has to be `Coll`'s companion (its module class).
        let Some(comp) = st.companion_module(*coll) else {
            continue;
        };
        if st.module_class_of(comp) != st.get(id).owner {
            continue;
        }
        work.push((id, *coll, args.len()));
    }
    for (id, coll, arity) in work {
        let names: &[&str] = if arity == 2 { &["K", "V"] } else { &["A"] };
        let tps: Vec<SymbolId> = names.iter().map(|n| type_param(st, id, n)).collect();
        let targs: Vec<Type> = tps.iter().map(|t| Type::TypeParam(*t)).collect();
        st.get_mut(id).tparams = tps;
        st.get_mut(id).ty = Type::Method {
            paramss: vec![vec![]],
            ret: Box::new(Type::Class {
                sym: coll,
                args: targs,
            }),
        };
    }
}
