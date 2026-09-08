//! Lower-bounded (`[B >: A]`) signatures for prelude members.
//!
//! Scala 2.13's immutable collections widen their element type on the "add one
//! element" operations: `def ::[B >: A](elem: B): List[B]`. Declaring the bound
//! here (rather than typing the parameter as `Any`) lets
//! `Typer::infer_method_tparams_in` join the argument type with the receiver's
//! element type, so `Circle(1) :: Rect(2, 3) :: Nil` is a `List[Shape]` instead
//! of a `List[Circle]`.
//!
//! Erasure is unchanged: `(B)List[B]` and the previous `(Any)List[A]` both
//! erase to `(Ljava/lang/Object;)Lscala/collection/immutable/List;`, so neither
//! the private runtime nor the real scala-library ABI is affected.

use crate::symbol::{SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

pub(crate) fn install(st: &mut SymbolTable) {
    install_map_add(st);
    let list = st.list_sym;
    let Some(elem) = st.get(list).tparams.first().copied() else {
        return;
    };
    let list_of = |b: SymbolId| Type::Class {
        sym: list,
        args: vec![Type::TypeParam(b)],
    };
    for m in members_named(st, list, "::") {
        let b = add_lower_bounded_tparam(st, m, "B", Type::TypeParam(elem));
        st.get_mut(m).ty = Type::Method {
            paramss: vec![vec![Type::TypeParam(b)]],
            ret: Box::new(list_of(b)),
        };
    }
    install_ordering_family(st, list, elem);
    install_membership(st, list, elem);
    install_reductions(st, list, elem);
}

/// `sorted` / `min` / `max` / `sum` / `product` on `List`.
///
/// nsc declares all five with a lower-bounded type parameter that occurs only
/// in the implicit clause:
///
/// ```text
/// def sorted[B >: A](implicit ord: Ordering[B]): C
/// def max[B >: A](implicit ord: Ordering[B]): A
/// def sum[B >: A](implicit num: Numeric[B]): B
/// ```
///
/// `prelude_seq`'s hand-written versions dropped the parameter and typed the
/// evidence as `Ordering[A]` / `Numeric[A]`, so `xs.sorted(AA.toOrdering)` with
/// `AA >: A` — cats writes it six times — was rejected with `found:
/// Ordering[AA] required: Ordering[A]`. Every other collection reaches these
/// through the pickle, which now keeps the parameter; `List` is the one class
/// whose declaration is ours.
///
/// Erasure is unchanged. `Ordering[B]` and `Ordering[A]` both erase to
/// `Lscala/math/Ordering;`, and `sum`'s result goes from `A` to `B`, which are
/// both `Ljava/lang/Object;`.
///
/// Under `--no-scala-library` none of the five is declared at all (see
/// `prelude_seq`'s `add_list_core_private`), so this is a no-op there.
fn install_ordering_family(st: &mut SymbolTable, list: SymbolId, elem: SymbolId) {
    let ordering = crate::classpath::find_by_jvm(st, "scala/math/Ordering");
    let numeric = crate::classpath::find_by_jvm(st, "scala/math/Numeric");
    let list_a = Type::Class {
        sym: list,
        args: vec![Type::TypeParam(elem)],
    };
    // (name, evidence class, result)
    let mut jobs: Vec<(&str, SymbolId, Ret)> = Vec::new();
    if let Some(ord) = ordering {
        jobs.push(("sorted", ord, Ret::Fixed(list_a)));
        jobs.push(("min", ord, Ret::Elem));
        jobs.push(("max", ord, Ret::Elem));
    }
    if let Some(num) = numeric {
        jobs.push(("sum", num, Ret::Widened));
        jobs.push(("product", num, Ret::Widened));
    }
    // `toArray[B >: A](implicit ct: ClassTag[B]): Array[B]` is the sixth of
    // exactly this shape and the probe found it: `dogs.toArray(ctAnimal)` was
    // `found: Array[B] required: Array[Animal]`, `B` never instantiated. Two
    // candidates were in scope -- `prelude_seq`'s monomorphic
    // `(implicit ClassTag[A]): Array[A]`, which the `ClassTag[Animal]`
    // argument does not fit, and the pickled `IterableOnceOps.toArray`, which
    // does. Rewriting ours to nsc's shape leaves one, and the instantiation
    // `agent/lowerbound` added for `sorted` then settles `B` from the
    // argument. `Array[B]`'s erasure is `Ljava/lang/Object;` exactly as
    // `Array[A]`'s was (`erasure.rs`'s `array_elem_is_abstract`).
    if let Some(ct) = crate::classpath::find_by_jvm(st, "scala/reflect/ClassTag") {
        jobs.push(("toArray", ct, Ret::ArrayOfWidened));
    }
    for (name, evidence, ret) in jobs {
        for m in members_named(st, list, name) {
            // Only the nullary-plus-one-implicit-clause shape is ours to
            // rewrite; anything else under this name was declared elsewhere.
            let Type::Method { paramss, .. } = st.get(m).ty.clone() else {
                continue;
            };
            if paramss.len() != 1 || paramss[0].len() != 1 {
                continue;
            }
            let b = add_lower_bounded_tparam(st, m, "B", Type::TypeParam(elem));
            let ev = Type::Class {
                sym: evidence,
                args: vec![Type::TypeParam(b)],
            };
            let result = match &ret {
                Ret::Fixed(t) => t.clone(),
                Ret::Elem => Type::TypeParam(elem),
                Ret::Widened => Type::TypeParam(b),
                Ret::ArrayOfWidened => Type::Array(Box::new(Type::TypeParam(b))),
            };
            set_single_implicit_param(st, m, ev.clone(), result.clone());
        }
    }
}

/// `contains[A1 >: A](elem: A1)` and `indexOf[B >: A](elem: B)` on `List`.
///
/// nsc puts the bound on the *searched-for* element, not on the receiver's:
///
/// ```text
/// def contains[A1 >: A](elem: A1): Boolean
/// def indexOf[B >: A](elem: B, from: Int = 0): Int
/// ```
///
/// so `List[Dog].contains(cat)` and even `List[Dog].contains(1)` compile —
/// `A1` is solved at the join, `Animal` and `Any` respectively. `prelude_seq`
/// wrote both as `(A)`, and the typer reported `no matching overload for
/// (Dog)Boolean with arguments (Cat)`: a monomorphic signature reads like a
/// missing alternative.
///
/// `lastIndexOf` and `indexOfSlice` are *not* touched here, and the probe
/// confirms they already behave: `prelude_seq` does not declare them, so they
/// arrive through the pickle with their bound intact. Only the members whose
/// declaration is ours diverge.
///
/// Erasure is unchanged. `A1`'s and `B`'s upper bound is `Any`, and `A`'s is
/// too, so `contains` stays `(Ljava/lang/Object;)Z` and `indexOf` stays
/// `(Ljava/lang/Object;)I`. That holds for the private runtime's `List`
/// classfile as well, which is why `contains` is widened in both modes;
/// `indexOf` exists only under `library_abi` and is simply absent here
/// otherwise.
fn install_membership(st: &mut SymbolTable, list: SymbolId, elem: SymbolId) {
    let ta = Type::TypeParam(elem);
    for m in members_named(st, list, "contains") {
        if !is_mono_one_arg(st, m, &ta) {
            continue;
        }
        let b = add_lower_bounded_tparam(st, m, "A1", ta.clone());
        st.get_mut(m).ty = Type::Method {
            paramss: vec![vec![Type::TypeParam(b)]],
            ret: Box::new(Type::Boolean),
        };
    }
    let mut widened_index_of = false;
    for m in members_named(st, list, "indexOf") {
        if !is_mono_one_arg(st, m, &ta) {
            continue;
        }
        let b = add_lower_bounded_tparam(st, m, "B", ta.clone());
        st.get_mut(m).ty = Type::Method {
            paramss: vec![vec![Type::TypeParam(b)]],
            ret: Box::new(Type::Int),
        };
        widened_index_of = true;
    }
    // `indexOf`'s `from` is a default argument in nsc, which reaches a call
    // site as a second overload here. Only added where the one-argument form
    // was found, so `--no-scala-library` (which declares no `indexOf` at all)
    // keeps reporting `indexOf is not a member of List[A]`.
    if widened_index_of
        && !members_named(st, list, "indexOf")
            .iter()
            .any(|&m| arity(st, m) == 2)
    {
        let m = st.alloc("indexOf", list, SymKind::Method, Flags::FINAL, "");
        let b = add_lower_bounded_tparam(st, m, "B", ta.clone());
        st.get_mut(m).ty = Type::Method {
            paramss: vec![vec![Type::TypeParam(b), Type::Int]],
            ret: Box::new(Type::Int),
        };
    }
}

/// `reduce` / `reduceLeft` / `reduceRight` on `List`.
///
/// ```text
/// def reduce     [B >: A](op: (B, B) => B): B
/// def reduceLeft [B >: A](op: (B, A) => B): B
/// def reduceRight[B >: A](op: (A, B) => B): B
/// ```
///
/// `prelude_seq` wrote all three as `((A, A) => A): A`, so
/// `dogs.reduceLeft((acc: Animal, d: Dog) => acc)` was `type mismatch; found:
/// (Animal, Dog) => Animal required: (A, A) => A`. The two asymmetric ones are
/// the reason this is not a one-line copy of `contains`: `reduceLeft`'s
/// accumulator is the widened side and its element is not, and `reduceRight`
/// is the mirror.
///
/// **Erasure and unboxing.** This is the member the parameter's move from `A`
/// to `B` could actually have changed, because the parameter is a *function*
/// of the widened type rather than a value of it. It does not: `B`'s upper
/// bound is `Any`, `A`'s is too, `Function2` erases to `scala/Function2`
/// whatever its arguments, so the descriptor stays
/// `(Lscala/Function2;)Ljava/lang/Object;` and the result is unboxed at the
/// call site exactly as before. `crates/cli/tests/preludelb.rs` checks this
/// against real scalac with `javap -c`, at `List[Int]` and at a widened
/// `List[Int].reduce[Any]`, because a member that type-checks and mis-boxes is
/// worse than one that is refused.
///
/// `reduceOption` / `reduceLeftOption` / `reduceRightOption` are not declared
/// by `prelude_seq` and reach `List` through the pickle already widened; the
/// probe confirms they take the same calls scalac takes.
fn install_reductions(st: &mut SymbolTable, list: SymbolId, elem: SymbolId) {
    let ta = Type::TypeParam(elem);
    // (name, which side of `op` stays at the element type)
    let jobs: [(&str, Side); 3] = [
        ("reduce", Side::Neither),
        ("reduceLeft", Side::Right),
        ("reduceRight", Side::Left),
    ];
    for (name, side) in jobs {
        for m in members_named(st, list, name) {
            let Type::Method { paramss, .. } = st.get(m).ty.clone() else {
                continue;
            };
            if paramss.len() != 1 || paramss[0].len() != 1 {
                continue;
            }
            // Only the `(A, A) => A` shape this module is here to widen.
            let Type::Function { params, .. } = &paramss[0][0] else {
                continue;
            };
            if params.len() != 2 {
                continue;
            }
            let b = add_lower_bounded_tparam(st, m, "B", ta.clone());
            let tb = Type::TypeParam(b);
            let (left, right) = match side {
                Side::Neither => (tb.clone(), tb.clone()),
                Side::Right => (tb.clone(), ta.clone()),
                Side::Left => (ta.clone(), tb.clone()),
            };
            st.get_mut(m).ty = Type::Method {
                paramss: vec![vec![Type::Function {
                    params: vec![left, right],
                    ret: Box::new(tb.clone()),
                }]],
                ret: Box::new(tb),
            };
        }
    }
}

/// Which operand of `op` keeps the receiver's element type `A`.
#[derive(Clone, Copy)]
enum Side {
    Neither,
    Left,
    Right,
}

/// `+[V1 >: V](kv: (K, V1)): Map[K, V1]` and
/// `updated[V1 >: V](key: K, value: V1): Map[K, V1]` on
/// `scala.collection.immutable.Map`.
///
/// `prelude_immutcoll2` writes `+` as `((K, V)): Map[K, V]`, so cats'
/// `WrappedMutableMapBase.scala:28` — a `Map[K, V] + ((k, v1))` with `v1` of a
/// supertype — was `no matching overload for (Tuple2[K, V])Map[K, V]`.
/// `updated` was `(Any, Any): Map[K, V]`, which *accepts* the same call and
/// then hands back the un-widened `Map[K, V]`: the probe caught
/// `val x: Dog = md.updated("k", cat).apply("k")` compiling here and being
/// rejected by scalac. Map is covariant in `V`, so nothing downstream of an
/// ascription to `Map[K, Animal]` could have noticed.
///
/// The key parameter is left exactly as it was — `Any` rather than `K` — which
/// is the same deliberate approximation `prelude_ovl3::widen_map_get_or_else`
/// records for `getOrElse`. It is a real divergence (`md.updated(1, dog)` is
/// accepted here and rejected by nsc) and it is not this slice's: tightening
/// it is a change to five members at once and has nothing to do with the lower
/// bound.
///
/// Erasure is unchanged: `V1` erases to `java/lang/Object` exactly as `V` did.
fn install_map_add(st: &mut SymbolTable) {
    let Some(map) = crate::classpath::find_by_jvm(st, "scala/collection/immutable/Map") else {
        return;
    };
    let tps = st.get(map).tparams.clone();
    if tps.len() != 2 {
        return;
    }
    let tk = Type::TypeParam(tps[0]);
    let tv = Type::TypeParam(tps[1]);
    let Some(tuple2) = crate::classpath::find_by_jvm(st, "scala/Tuple2") else {
        return;
    };
    let map_of = |v: &Type| Type::Class {
        sym: map,
        args: vec![tk.clone(), v.clone()],
    };
    let pair_v = Type::Class {
        sym: tuple2,
        args: vec![tk.clone(), tv.clone()],
    };
    for m in members_named(st, map, "+") {
        let Type::Method { paramss, .. } = st.get(m).ty.clone() else {
            continue;
        };
        if paramss.len() != 1 || paramss[0].len() != 1 || paramss[0][0] != pair_v {
            continue;
        }
        let v1 = add_lower_bounded_tparam(st, m, "V1", tv.clone());
        let tv1 = Type::TypeParam(v1);
        st.get_mut(m).ty = Type::Method {
            paramss: vec![vec![Type::Class {
                sym: tuple2,
                args: vec![tk.clone(), tv1.clone()],
            }]],
            ret: Box::new(map_of(&tv1)),
        };
    }
    for m in members_named(st, map, "updated") {
        let Type::Method { paramss, .. } = st.get(m).ty.clone() else {
            continue;
        };
        if paramss.len() != 1 || paramss[0].len() != 2 || paramss[0][1] != Type::Any {
            continue;
        }
        let key = paramss[0][0].clone();
        let v1 = add_lower_bounded_tparam(st, m, "V1", tv.clone());
        let tv1 = Type::TypeParam(v1);
        st.get_mut(m).ty = Type::Method {
            paramss: vec![vec![key, tv1.clone()]],
            ret: Box::new(map_of(&tv1)),
        };
    }
}

/// `method` takes exactly one explicit argument, of type `want`, and has no
/// type parameters yet — the shape `prelude_seq`'s `simple` leaves behind.
fn is_mono_one_arg(st: &SymbolTable, method: SymbolId, want: &Type) -> bool {
    if !st.get(method).tparams.is_empty() {
        return false;
    }
    match &st.get(method).ty {
        Type::Method { paramss, .. } => {
            paramss.len() == 1 && paramss[0].len() == 1 && &paramss[0][0] == want
        }
        _ => false,
    }
}

fn arity(st: &SymbolTable, method: SymbolId) -> usize {
    match &st.get(method).ty {
        Type::Method { paramss, .. } => paramss.iter().map(|c| c.len()).sum(),
        _ => 0,
    }
}

/// What the rewritten member returns: the receiver's own collection, its
/// element type, or the widened `B`.
enum Ret {
    Fixed(Type),
    Elem,
    Widened,
    ArrayOfWidened,
}

/// Replace `method`'s parameters with one implicit clause holding a single
/// parameter of type `ev`, and its result with `ret`.
///
/// The parameter *symbols* carry `Flags::IMPLICIT`; implicit search reads them
/// rather than the `Type::Method`, so rewriting only the type would leave the
/// evidence looking like an ordinary argument.
fn set_single_implicit_param(st: &mut SymbolTable, method: SymbolId, ev: Type, ret: Type) {
    let p = st.alloc(
        "evidence$1",
        method,
        SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(p).ty = ev.clone();
    st.get_mut(method).params = vec![p];
    st.get_mut(method).paramss = vec![vec![p]];
    st.get_mut(method).ty = Type::Method {
        paramss: vec![vec![ev]],
        ret: Box::new(ret),
    };
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
/// already had.
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
