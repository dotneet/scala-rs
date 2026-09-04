//! Failure to summon `Equiv[Int]` (left over from `agent/ordsummon`).
//!
//! The real ABI (`javap -p -s scala.math.Ordering` / `PartialOrdering` / `Equiv`):
//!
//! ```text
//! interface scala.math.Ordering<T>        extends java.util.Comparator<T>, scala.math.PartialOrdering<T>
//! interface scala.math.PartialOrdering<T>  extends scala.math.Equiv<T>
//! interface scala.math.Equiv<T>            extends java.io.Serializable
//! ```
//!
//! `Equiv` appeared nowhere in the prelude (`Ordering` is hand-built in full by
//! `add_ordering` in `crates/typer/src/prelude.rs`, but there is no counterpart for
//! `Equiv`). The **type** `Equiv[Int]` itself was found by `expose_unqualified` in
//! `check.rs` through the real `scala` package object's pickled alias
//! (`type Equiv[T] = scala.math.Equiv[T]`), but the `PickleSupply::complete` used
//! there is the lightweight version that reads "only names and signatures out of a
//! pickle fragment" and does not carry inheritance
//! (see the documentation of `attach_classpath_parents` in
//! `crates/typer/src/classpath.rs`). Inheritance is meant to be supplied by
//! `pickle_supply::ensure_parents` "only once a reference has failed", and that is
//! only ever called from prefix resolution for `import x._`. As a result:
//!
//! ```scala
//! val e: Equiv[Int] = Ordering.Int             // real scalac: OK (weakening assignment)
//! val p: PartialOrdering[Int] = Ordering.Int    // real scalac: OK
//! implicitly[Equiv[Int]]                        // real scalac: OK
//! ```
//!
//! all failed under scala-rs: the first two as a `type mismatch`, because
//! `Ordering[Int]` did not carry `PartialOrdering` / `Equiv` as parents; the third as
//! `could not find implicit value`, because `object Equiv` carried no implicit
//! instance at all (real scalac picks `object Equiv`'s own dedicated instance
//! `Equiv$Int$` rather than `Ordering.Int` -- confirmed with
//! `implicitly[Equiv[Int]].getClass.getName`).
//!
//! `Ordering` / `Numeric` / `Integral` / `Fractional` (`prelude_seq.rs` /
//! `prelude_numhier.rs`) plug the same hole by "building their own class plus
//! companion module at prelude time and entering it into the current scope, without
//! waiting for the jar to load". `Equiv` and `PartialOrdering` get the same treatment:
//! once they are built and `enter_in_current`ed here, `expose_unqualified` -- which
//! would otherwise resolve them through the jar later -- does not fire because they
//! are "already in scope", and only these prelude symbols are used. The members
//! (`equiv` / `fromComparator` / `by` / `TupleN` and so on) are supplied on demand by
//! `pickle_supply` as long as the `jvm_name` matches the real class (the same way
//! `Ordering`'s `lt` / `gt` / `lteq` / `gteq` / `max` / `min` still work).

use scala_rs_parser::{Flags, SymbolId, Type};

use crate::symbol::{SymKind, SymbolTable};

pub(crate) fn install(st: &mut SymbolTable, library_abi: bool) {
    if !library_abi {
        // The private runtime has no `scala/math/Equiv` / `PartialOrdering`
        // classfile. By building nothing here, `Equiv[Int]` stays the diagnostic
        // `not found: value Equiv` (no stubs -- `.agent-brief.md`).
        return;
    }
    let Some(ordering) = crate::classpath::find_by_jvm(st, "scala/math/Ordering") else {
        return;
    };
    let math = crate::classpath::ensure_package(st, "scala/math");
    let equiv = ensure_equiv(st, math);
    let partial = ensure_partial_ordering(st, math, equiv);
    add_parent(st, ordering, partial);
    add_equiv_instances(st, equiv);
}

/// Build `trait Equiv[T]` and its companion module inside the prelude and enter both
/// into the current scope, as a type and as a term (doing nothing if already there).
fn ensure_equiv(st: &mut SymbolTable, math: SymbolId) -> SymbolId {
    if let Some(id) = crate::classpath::find_by_jvm(st, "scala/math/Equiv") {
        enter_type(st, "Equiv", id);
        if let Some(m) = st.companion_module(id) {
            enter_term(st, "Equiv", m);
        }
        return id;
    }
    let equiv = crate::prelude::iface(st, math, "Equiv", "scala/math/Equiv");
    let t = crate::prelude::type_param(st, equiv, "T");
    st.get_mut(equiv).tparams = vec![t];
    crate::prelude::method(
        st,
        equiv,
        "equiv",
        vec![Type::TypeParam(t), Type::TypeParam(t)],
        Type::Boolean,
        crate::symbol::Intrinsic::None,
    );
    let m = crate::prelude::module(st, math, "Equiv", "scala/math/Equiv$");
    enter_type(st, "Equiv", equiv);
    enter_term(st, "Equiv", m);
    equiv
}

/// Build `trait PartialOrdering[T] extends Equiv[T]` inside the prelude and enter it
/// into the current scope as a type (if it is already there, just add `Equiv` as a
/// parent). Real scalac has no summonable instance either, so no companion module is
/// built (`implicitly[PartialOrdering[Int]]` stays `could not find implicit value`,
/// just as under real scalac).
fn ensure_partial_ordering(st: &mut SymbolTable, math: SymbolId, equiv: SymbolId) -> SymbolId {
    if let Some(id) = crate::classpath::find_by_jvm(st, "scala/math/PartialOrdering") {
        enter_type(st, "PartialOrdering", id);
        add_parent(st, id, equiv);
        return id;
    }
    let partial = crate::prelude::iface(st, math, "PartialOrdering", "scala/math/PartialOrdering");
    let t = crate::prelude::type_param(st, partial, "T");
    st.get_mut(partial).tparams = vec![t];
    add_parent(st, partial, equiv);
    enter_type(st, "PartialOrdering", partial);
    partial
}

fn enter_type(st: &mut SymbolTable, name: &str, id: SymbolId) {
    let already = st
        .lookup(name)
        .into_iter()
        .any(|s| s == id && st.get(s).kind == SymKind::Class);
    if !already {
        st.enter_in_current(name, id);
    }
}

fn enter_term(st: &mut SymbolTable, name: &str, id: SymbolId) {
    if st.lookup(name).into_iter().any(|s| s == id) {
        return;
    }
    st.enter_in_current(name, id);
}

/// `child[T] extends parent[T]`, passing `child`'s first type parameter straight
/// through. Does nothing if the parent is already there (the same shape as
/// `prelude_numhier::add_parent` -- duplicated because this is a separate module).
fn add_parent(st: &mut SymbolTable, child: SymbolId, parent: SymbolId) {
    if st.get(child).kind != SymKind::Class {
        return;
    }
    if st
        .get(child)
        .parents
        .iter()
        .any(|p| matches!(p, Type::Class { sym, .. } if *sym == parent))
    {
        return;
    }
    let args = match st.get(child).tparams.first().copied() {
        Some(tp) if !st.get(parent).tparams.is_empty() => vec![Type::TypeParam(tp)],
        _ => Vec::new(),
    };
    st.get_mut(child)
        .parents
        .push(Type::Class { sym: parent, args });
}

/// `object Equiv`'s implicit instances. jar: `scala/math/Equiv$<Name>$`.
fn add_equiv_instances(st: &mut SymbolTable, equiv: SymbolId) {
    let Some(equiv_mod) = st.companion_module(equiv) else {
        return;
    };
    let equiv_cls = st.module_class_of(equiv_mod);
    let big_int = crate::classpath::find_by_jvm(st, "scala/math/BigInt")
        .map(|sym| Type::Class { sym, args: vec![] });
    let big_dec = crate::classpath::find_by_jvm(st, "scala/math/BigDecimal")
        .map(|sym| Type::Class { sym, args: vec![] });
    let symbol = crate::classpath::find_by_jvm(st, "scala/Symbol")
        .map(|sym| Type::Class { sym, args: vec![] });
    let table: Vec<(&str, &str, Option<Type>)> = vec![
        ("Unit", "scala/math/Equiv$Unit$", Some(Type::Unit)),
        ("Boolean", "scala/math/Equiv$Boolean$", Some(Type::Boolean)),
        ("Byte", "scala/math/Equiv$Byte$", Some(Type::Byte)),
        ("Char", "scala/math/Equiv$Char$", Some(Type::Char)),
        ("Short", "scala/math/Equiv$Short$", Some(Type::Short)),
        ("Int", "scala/math/Equiv$Int$", Some(Type::Int)),
        ("Long", "scala/math/Equiv$Long$", Some(Type::Long)),
        ("BigInt", "scala/math/Equiv$BigInt$", big_int),
        ("BigDecimal", "scala/math/Equiv$BigDecimal$", big_dec),
        ("String", "scala/math/Equiv$String$", Some(Type::String)),
        ("Symbol", "scala/math/Equiv$Symbol$", symbol),
        // 2.13: `Equiv.Double` / `Equiv.Float` became namespace objects holding
        // `StrictEquiv` / `IeeeEquiv`, and the ones picked as implicits are the
        // deprecated versions (see the module docs).
        (
            "DeprecatedDoubleEquiv",
            "scala/math/Equiv$DeprecatedDoubleEquiv$",
            Some(Type::Double),
        ),
        (
            "DeprecatedFloatEquiv",
            "scala/math/Equiv$DeprecatedFloatEquiv$",
            Some(Type::Float),
        ),
    ];
    for (name, jvm, ty) in table {
        let Some(ty) = ty else { continue };
        add_implicit_instance(st, equiv_cls, equiv, name, jvm, ty);
    }
    let known: Vec<SymbolId> = st.get(equiv_mod).members.clone();
    for m in st.get(equiv_cls).members.clone() {
        if !known.contains(&m) {
            st.get_mut(equiv_mod).members.push(m);
        }
    }
}

/// Create `implicit object <name> extends Equiv[<arg>]` (`<jvm>.MODULE$`).
/// Does nothing if a member of that name already exists (the same shape as
/// `prelude_seq::add_implicit_instance` -- duplicated because this is a separate module).
fn add_implicit_instance(
    st: &mut SymbolTable,
    equiv_cls: SymbolId,
    equiv: SymbolId,
    name: &str,
    jvm: &str,
    arg: Type,
) {
    if st
        .get(equiv_cls)
        .members
        .iter()
        .copied()
        .any(|m| st.get(m).name == name)
    {
        return;
    }
    let cls = st.alloc(
        format!("{name}$"),
        equiv_cls,
        SymKind::ModuleClass,
        Flags::MODULE.with(Flags::FINAL),
        jvm,
    );
    let m = st.alloc(name, equiv_cls, SymKind::Module, Flags::MODULE, jvm);
    st.get_mut(m).flags = st.get(m).flags.with(Flags::IMPLICIT);
    let ty = Type::Class {
        sym: equiv,
        args: vec![arg],
    };
    st.get_mut(m).ty = ty.clone();
    st.get_mut(cls).ty = Type::ModuleRef(cls);
    st.get_mut(cls).parents = vec![ty];
}
