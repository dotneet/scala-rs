//! The `[B >: A]` widening members, the collection hierarchy edges the
//! concrete `Hash*`/`LinkedHash*` classes were missing, and one absent
//! `StringBuilder` constructor.
//!
//! All three showed up in slick as `no matching overload`, which is the
//! message the typer prints when a *single* candidate's parameters reject the
//! arguments — so a monomorphic signature reads like a missing alternative:
//!
//!   - `Option.getOrElse` was declared `(default: => A): A` with a note in
//!     `prelude_coll` admitting the same shortcut for `Map.getOrElse`
//!     ("modeled monomorphically as V"). nsc declares
//!     `getOrElse[B >: A](default: => B): B`, so
//!     `(o: Option[Sub]).getOrElse(base)` is a `Base`; monomorphically it was
//!     an argument of type `Base` handed to a parameter of type `Sub`.
//!     `Infer::infer_method_tparams_in` already joins the argument type with a
//!     lower bound (that is what `prelude_lowbound` relies on for `List.::`),
//!     so declaring the bound is the whole fix.
//!   - `scala.collection.mutable.HashSet` / `HashMap` extended `AnyRef` and
//!     nothing else, so slick's `def containsSymbol(tss: collection.Set[…])`
//!     rejected the `mutable.HashSet` it is always called with.
//!   - `StringBuilder`'s constructor table simply had no `(Int, String)`.
//!
//! Erasure is unaffected: `B` and `V1` are type parameters, so every widened
//! signature still erases to the descriptor the previous one did.

use crate::prelude::prelude_method;
use crate::symbol::{Intrinsic, SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

pub(crate) fn install(st: &mut SymbolTable, library_abi: bool) {
    widen_option(st);
    widen_map_get_or_else(st);
    if library_abi {
        add_string_builder_ctor(st);
    }
}

/// `new StringBuilder(initCapacity: Int, initValue: String)`
/// (`slick/util/TableDump.scala:50`).
///
/// `prelude_text`'s constructor table has `()` / `(Int)` / `(String)`. The
/// two-argument one is `library_abi`-only for the same reason that whole
/// table is: `--no-scala-library` compiles `scala.collection.mutable.
/// StringBuilder` down to `java.lang.StringBuilder`, which has no
/// `(int, String)` constructor at all.
fn add_string_builder_ctor(st: &mut SymbolTable) {
    let Some(sb) = crate::classpath::find_by_jvm(st, "scala/collection/mutable/StringBuilder")
    else {
        return;
    };
    let already = st
        .get(sb)
        .members
        .iter()
        .copied()
        .any(|m| st.get(m).name == "<init>" && flat_params(&st.get(m).ty) == 2);
    if already {
        return;
    }
    let sb_t = Type::Class {
        sym: sb,
        args: vec![],
    };
    prelude_method(
        st,
        sb,
        "<init>",
        vec![Type::Int, Type::String],
        sb_t,
        Intrinsic::None,
    );
}

fn flat_params(ty: &Type) -> usize {
    match ty {
        Type::Method { paramss, .. } => paramss.iter().map(|c| c.len()).sum(),
        _ => 0,
    }
}

/// The hierarchy edges. Run after `prelude_hier::install`, which is what
/// builds the `collection.Set` / `collection.Map` ends of them.
pub(crate) fn install_hierarchy(st: &mut SymbolTable) {
    // (child, parent) — the child passes its own type parameters straight
    // through, so both sides must have the same number of them.
    const EDGES: &[(&str, &str)] = &[
        (
            "scala/collection/mutable/HashSet",
            "scala/collection/mutable/Set",
        ),
        (
            "scala/collection/mutable/LinkedHashSet",
            "scala/collection/mutable/Set",
        ),
        (
            "scala/collection/mutable/HashMap",
            "scala/collection/mutable/Map",
        ),
        (
            "scala/collection/mutable/LinkedHashMap",
            "scala/collection/mutable/Map",
        ),
    ];
    for (child_jvm, parent_jvm) in EDGES {
        let (Some(child), Some(parent)) = (
            crate::classpath::find_by_jvm(st, child_jvm),
            crate::classpath::find_by_jvm(st, parent_jvm),
        ) else {
            continue;
        };
        let cps = st.get(child).tparams.clone();
        let pps = st.get(parent).tparams.clone();
        if cps.is_empty() || cps.len() != pps.len() {
            continue;
        }
        let applied = Type::Class {
            sym: parent,
            args: cps.iter().map(|&p| Type::TypeParam(p)).collect(),
        };
        // Keep `AnyRef` last, and never install the edge twice.
        if st
            .get(child)
            .parents
            .iter()
            .any(|p| matches!(p, Type::Class { sym, .. } if *sym == parent))
        {
            continue;
        }
        st.get_mut(child)
            .parents
            .retain(|p| !matches!(p, Type::AnyRef));
        st.get_mut(child).parents.push(applied);
    }
}

/// `getOrElse[B >: A](default: => B): B` and
/// `orElse[B >: A](alternative: => Option[B]): Option[B]`.
fn widen_option(st: &mut SymbolTable) {
    let o = st.option_sym;
    if o.is_none() {
        return;
    }
    let Some(&a) = st.get(o).tparams.first() else {
        return;
    };
    let ta = Type::TypeParam(a);
    for m in members_named(st, o, "getOrElse") {
        let b = add_lower_bounded_tparam(st, m, "B", ta.clone());
        let tb = Type::TypeParam(b);
        st.get_mut(m).ty = Type::Method {
            paramss: vec![vec![Type::ByName(Box::new(tb.clone()))]],
            ret: Box::new(tb),
        };
    }
    for m in members_named(st, o, "orElse") {
        let b = add_lower_bounded_tparam(st, m, "B", ta.clone());
        let opt_b = Type::Class {
            sym: o,
            args: vec![Type::TypeParam(b)],
        };
        st.get_mut(m).ty = Type::Method {
            paramss: vec![vec![Type::ByName(Box::new(opt_b.clone()))]],
            ret: Box::new(opt_b),
        };
    }
}

/// `getOrElse[V1 >: V](key: K, default: => V1): V1` on every `Map` the prelude
/// declares it on. The key parameter is left exactly as it was — the immutable
/// `Map` types it `Any`, which is a separate (deliberate) approximation.
fn widen_map_get_or_else(st: &mut SymbolTable) {
    for jvm in [
        "scala/collection/immutable/Map",
        "scala/collection/mutable/Map",
    ] {
        let Some(map) = crate::classpath::find_by_jvm(st, jvm) else {
            continue;
        };
        let tps = st.get(map).tparams.clone();
        if tps.len() != 2 {
            continue;
        }
        let tv = Type::TypeParam(tps[1]);
        for m in members_named(st, map, "getOrElse") {
            // Only the monomorphic shape this module is here to fix.
            let Type::Method { paramss, ret } = st.get(m).ty.clone() else {
                continue;
            };
            if paramss.len() != 1 || paramss[0].len() != 2 || *ret != tv {
                continue;
            }
            let key = paramss[0][0].clone();
            let v1 = add_lower_bounded_tparam(st, m, "V1", tv.clone());
            let tv1 = Type::TypeParam(v1);
            st.get_mut(m).ty = Type::Method {
                paramss: vec![vec![key, Type::ByName(Box::new(tv1.clone()))]],
                ret: Box::new(tv1),
            };
        }
    }
}

fn members_named(st: &SymbolTable, owner: SymbolId, name: &str) -> Vec<SymbolId> {
    st.get(owner)
        .members
        .iter()
        .copied()
        .filter(|&m| st.get(m).kind == SymKind::Method && st.get(m).name == name)
        .collect()
}

/// Give `method` a single type parameter `name` with `>: lo`, replacing any it
/// already had. (Same shape as `prelude_lowbound`'s helper; kept local so the
/// two slices do not have to share a file.)
fn add_lower_bounded_tparam(
    st: &mut SymbolTable,
    method: SymbolId,
    name: &str,
    lo: Type,
) -> SymbolId {
    let b = st.alloc(name, method, SymKind::TypeParam, Flags::EMPTY, "");
    st.get_mut(b).ty = Type::TypeParam(b);
    st.get_mut(b).bound_lo = Some(lo);
    st.get_mut(method).tparams = vec![b];
    b
}
