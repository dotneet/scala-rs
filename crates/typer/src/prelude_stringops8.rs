//! The `StringOps` members the library pickle cannot express on its own.
//!
//! Most of `StringOps` now arrives from `scala-library.jar`'s `ScalaSignature`:
//! `Check::search_extension` asks [`crate::pickle_supply`] to complete the
//! conversion *result* when the hand-written prelude has nothing for the
//! selected name. That covers ~30 members this file would otherwise have had
//! to spell out (`groupBy`, `sortBy`, `sliding`, `tails`, `permutations`, …).
//!
//! What it cannot cover is the **result-type overloads**. 2.13 `StringOps`
//! declares pairs that differ only in return type:
//!
//! ```text
//! public java.lang.String                   collect(scala.PartialFunction);
//! public <B> scala.collection.immutable.IndexedSeq<B> collect(scala.PartialFunction);
//! ```
//!
//! `pickle_supply::erased_desc` keys a member by its *parameter* erasure, so
//! it finds two candidates, cannot tell them apart, and declines
//! ("no unambiguous erased descriptor"). Declining is the honest thing for it
//! to do -- but the member then falls through to the lower-priority
//! `wrapString` conversion and comes back as a `WrappedString`/`IndexedSeq`,
//! so `"abcdef".collect { case c if c > 'c' => c }` returned `Vector(d, e, f)`
//! where scalac returns the `String` `"def"`. Wrong types are worse than a
//! missing member, so each such pair is declared here by hand, the same way
//! [`crate::prelude_strmap`] already declares the two `map`s: **two symbols**,
//! because `value_extension_desc` builds the descriptor from the symbol's own
//! result type, and folding them into one would call the `IndexedSeq` static
//! for a `Char`-returning function and `ClassCastException`.
//!
//! `withFilter` is here for a related reason: it returns a
//! `StringOps.WithFilter`, a *plain* class (not a value class) whose own
//! `map`/`flatMap` are the same doubled-erasure pairs. Completing only
//! `withFilter` from the pickle left `.map` on the result unresolvable, and
//! the call fell back to the `String` receiver and died with
//! `StringOps$WithFilter cannot be cast to java.lang.String`.
//!
//! **私有ランタイム（`--no-scala-library`）** には `StringOps` 自体が無いので、
//! `prelude_strmap` と同じく `library_abi` のときだけ入れる。

use crate::symbol::{SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

pub(crate) fn install(st: &mut SymbolTable, library_abi: bool) {
    if !library_abi {
        return;
    }
    let Some(so) = find_by_jvm(st, "scala/collection/StringOps") else {
        return;
    };
    let Some(idx) = find_indexed_seq(st) else {
        return;
    };
    let Some(pf) = find_partial_function(st) else {
        return;
    };
    add_collect(st, so, idx, pf);
    add_apply(st, so);
    add_add_string(st, so);
    add_with_filter(st, so, idx);
}

/// `collect(pf: PartialFunction[Char, Char]): String` and
/// `collect[B](pf: PartialFunction[Char, B]): IndexedSeq[B]`.
///
/// nsc 2.13.16 JVM:
/// `collect$extension(String, PartialFunction)String` and
/// `collect$extension(String, PartialFunction)IndexedSeq`.
fn add_collect(st: &mut SymbolTable, so: SymbolId, idx: SymbolId, pf: SymbolId) {
    if !st.lookup_member(so, "collect").is_empty() {
        return;
    }
    // The `String` one. More specific, so it wins for `Char => Char`.
    let m = st.alloc("collect", so, SymKind::Method, Flags::FINAL, "");
    let p = st.alloc("pf", m, SymKind::Term, Flags::PARAM, "");
    let pf_cc = Type::Class {
        sym: pf,
        args: vec![Type::Char, Type::Char],
    };
    st.get_mut(p).ty = pf_cc.clone();
    st.get_mut(m).params = vec![p];
    st.get_mut(m).paramss = vec![vec![p]];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![vec![pf_cc]],
        ret: Box::new(Type::String),
    };

    // The generic one.
    let g = st.alloc("collect", so, SymKind::Method, Flags::FINAL, "");
    let b = st.alloc("B", g, SymKind::TypeParam, Flags::EMPTY, "");
    st.get_mut(b).ty = Type::TypeParam(b);
    st.get_mut(g).tparams = vec![b];
    let tb = Type::TypeParam(b);
    let pf_cb = Type::Class {
        sym: pf,
        args: vec![Type::Char, tb.clone()],
    };
    let gp = st.alloc("pf", g, SymKind::Term, Flags::PARAM, "");
    st.get_mut(gp).ty = pf_cb.clone();
    st.get_mut(g).params = vec![gp];
    st.get_mut(g).paramss = vec![vec![gp]];
    st.get_mut(g).ty = Type::Method {
        paramss: vec![vec![pf_cb]],
        ret: Box::new(Type::Class {
            sym: idx,
            args: vec![tb],
        }),
    };
}

/// `apply(i: Int): Char`. nsc: `apply$extension(String, int)char`.
fn add_apply(st: &mut SymbolTable, so: SymbolId) {
    if !st.lookup_member(so, "apply").is_empty() {
        return;
    }
    let m = st.alloc("apply", so, SymKind::Method, Flags::FINAL, "");
    let p = st.alloc("i", m, SymKind::Term, Flags::PARAM, "");
    st.get_mut(p).ty = Type::Int;
    st.get_mut(m).params = vec![p];
    st.get_mut(m).paramss = vec![vec![p]];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![vec![Type::Int]],
        ret: Box::new(Type::Char),
    };
}

/// The three `addString` arities, all returning the `StringBuilder` they were
/// handed. The pickle declines these because `mutable.StringBuilder` reaches
/// the typer as a prelude class whose pickled shape does not line up.
fn add_add_string(st: &mut SymbolTable, so: SymbolId) {
    if !st.lookup_member(so, "addString").is_empty() {
        return;
    }
    let Some(sb) = find_string_builder(st) else {
        return;
    };
    let sbt = Type::Class {
        sym: sb,
        args: vec![],
    };
    for extra in 0..3 {
        // arities: (b), (b, sep), (b, start, sep, end)
        let n = match extra {
            0 => 0,
            1 => 1,
            _ => 3,
        };
        let m = st.alloc("addString", so, SymKind::Method, Flags::FINAL, "");
        let mut ps = vec![st.alloc("b", m, SymKind::Term, Flags::PARAM, "")];
        st.get_mut(ps[0]).ty = sbt.clone();
        let mut tys = vec![sbt.clone()];
        for k in 0..n {
            let p = st.alloc(format!("s{k}"), m, SymKind::Term, Flags::PARAM, "");
            st.get_mut(p).ty = Type::String;
            ps.push(p);
            tys.push(Type::String);
        }
        st.get_mut(m).params = ps.clone();
        st.get_mut(m).paramss = vec![ps];
        st.get_mut(m).ty = Type::Method {
            paramss: vec![tys],
            ret: Box::new(sbt.clone()),
        };
    }
}

/// `withFilter(p: Char => Boolean): StringOps.WithFilter`, plus the
/// `WithFilter` class itself.
///
/// `WithFilter` is a plain class, so its members are ordinary
/// `invokevirtual`s against `scala/collection/StringOps$WithFilter` -- no
/// `$extension` involved. Its `map` and `flatMap` are result-type overloads
/// like `StringOps`' own, so each gets two symbols.
fn add_with_filter(st: &mut SymbolTable, so: SymbolId, idx: SymbolId) {
    if !st.lookup_member(so, "withFilter").is_empty() {
        return;
    }
    let wf = alloc_class(
        st,
        st.scala_pkg,
        "StringOps$WithFilter",
        "scala/collection/StringOps$WithFilter",
    );

    // foreach[U](f: Char => U): Unit
    let fe = st.alloc("foreach", wf, SymKind::Method, Flags::EMPTY, "");
    let u = st.alloc("U", fe, SymKind::TypeParam, Flags::EMPTY, "");
    st.get_mut(u).ty = Type::TypeParam(u);
    st.get_mut(fe).tparams = vec![u];
    set_fn1_method(st, fe, "f", Type::Char, Type::TypeParam(u), Type::Unit);

    // map(f: Char => Char): String  /  map[B](f: Char => B): IndexedSeq[B]
    let m1 = st.alloc("map", wf, SymKind::Method, Flags::EMPTY, "");
    set_fn1_method(st, m1, "f", Type::Char, Type::Char, Type::String);
    let m2 = st.alloc("map", wf, SymKind::Method, Flags::EMPTY, "");
    let b2 = st.alloc("B", m2, SymKind::TypeParam, Flags::EMPTY, "");
    st.get_mut(b2).ty = Type::TypeParam(b2);
    st.get_mut(m2).tparams = vec![b2];
    let seq_b2 = Type::Class {
        sym: idx,
        args: vec![Type::TypeParam(b2)],
    };
    set_fn1_method(st, m2, "f", Type::Char, Type::TypeParam(b2), seq_b2);

    // flatMap(f: Char => String): String
    let f1 = st.alloc("flatMap", wf, SymKind::Method, Flags::EMPTY, "");
    set_fn1_method(st, f1, "f", Type::Char, Type::String, Type::String);

    // withFilter(p: Char => Boolean): WithFilter
    let wfty = Type::Class {
        sym: wf,
        args: vec![],
    };
    let w2 = st.alloc("withFilter", wf, SymKind::Method, Flags::EMPTY, "");
    set_fn1_method(st, w2, "p", Type::Char, Type::Boolean, wfty.clone());

    // StringOps.withFilter
    let w = st.alloc("withFilter", so, SymKind::Method, Flags::FINAL, "");
    set_fn1_method(st, w, "p", Type::Char, Type::Boolean, wfty);
}

/// Give `id` the shape `(f: P => R): Ret`, params and all.
fn set_fn1_method(st: &mut SymbolTable, id: SymbolId, pname: &str, p: Type, r: Type, ret: Type) {
    let fty = Type::Function {
        params: vec![p],
        ret: Box::new(r),
    };
    let a = st.alloc(pname, id, SymKind::Term, Flags::PARAM, "");
    st.get_mut(a).ty = fty.clone();
    st.get_mut(id).params = vec![a];
    st.get_mut(id).paramss = vec![vec![a]];
    st.get_mut(id).ty = Type::Method {
        paramss: vec![vec![fty]],
        ret: Box::new(ret),
    };
}

fn alloc_class(st: &mut SymbolTable, owner: SymbolId, name: &str, jvm: &str) -> SymbolId {
    if let Some(id) = find_by_jvm(st, jvm) {
        return id;
    }
    let id = st.alloc(name, owner, SymKind::Class, Flags::EMPTY, jvm);
    st.get_mut(id).ty = Type::Class {
        sym: id,
        args: vec![],
    };
    st.get_mut(id).parents = vec![Type::AnyRef];
    id
}

fn find_by_jvm(st: &SymbolTable, jvm: &str) -> Option<SymbolId> {
    crate::classpath::find_by_jvm(st, jvm)
}

fn find_indexed_seq(st: &SymbolTable) -> Option<SymbolId> {
    st.lookup_member(st.scala_pkg, "IndexedSeq")
        .into_iter()
        .find(|s| st.get(*s).jvm_name == "scala/collection/immutable/IndexedSeq")
}

fn find_partial_function(st: &SymbolTable) -> Option<SymbolId> {
    st.lookup_member(st.scala_pkg, "PartialFunction")
        .into_iter()
        .find(|s| st.get(*s).jvm_name == "scala/PartialFunction")
}

fn find_string_builder(st: &SymbolTable) -> Option<SymbolId> {
    find_by_jvm(st, "scala/collection/mutable/StringBuilder")
}
