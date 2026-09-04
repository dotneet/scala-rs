//! The parent-child relations of the `scala.math` type class hierarchy (`Numeric` /
//! `Integral` / `Fractional` / `Ordering`), and the types of `object Numeric`'s
//! implicit instances.
//!
//! Called on a single line from `install_prelude` in `crates/typer/src/prelude.rs`.
//! Split into a new module to avoid merge conflicts (`.agent-brief.md`'s policy).
//!
//! The prelude only synthesised `scala.math.Numeric` as "a box for resolving the
//! implicit arguments of `sum` / `product`"; it did not mirror the real ABI's
//! `interface scala.math.Numeric<T> extends scala.math.Ordering<T>`
//! (confirmed with `javap`). As a result
//!
//! ```scala
//! class B[T](implicit ct: ClassTag[T], ord: Ordering[T])
//! class N[T](implicit tag: ClassTag[T], num: Numeric[T]) extends B[T]()(tag, num)
//! ```
//!
//! (slick `ScalaNumericType`) did not admit `Numeric[T]` as `Ordering[T]` and came
//! out as `no matching overload for constructor`. Member resolution alone did hit by
//! another route through `lookup_member`, so the symptom showed up in the confusing
//! shape of "only subtyping fails".
//!
//! # `Integral` / `Fractional` (`agent/integral`)
//!
//! `List.range(0, 5)` / `Vector.range` / `Seq.range` are
//! `IterableFactory#range[A](start: A, end: A)(implicit ord: Integral[A])`, and came
//! out as `no implicit: could not find implicit value of type Integral[Int]`. Two
//! causes:
//!
//! 1. `Integral` / `Fractional` are not in the symbol table at prelude time. When the
//!    source mentions the name, `pickle_supply` raises a stub and attaches the parent
//!    (`Numeric`) from the pickle **only when member resolution fails**. That is too
//!    late for the subtyping check, so the `Integral[T] <: Numeric[T]` edge stayed
//!    missing.
//! 2. `object Numeric`'s implicit instances were given `Numeric[Int]`. In the real
//!    ABI `Numeric$IntIsIntegral$` implements
//!    `Numeric$IntIsIntegral extends Integral<Object>`, so `Integral[Int]` is the
//!    correct one.
//!
//! The shapes confirmed with `javap -p -s /tmp/scala-rs-lib/scala-library-2.13.16.jar`:
//!
//! ```text
//! interface scala.math.Numeric<T>    extends scala.math.Ordering<T>
//! interface scala.math.Integral<T>   extends scala.math.Numeric<T>
//! interface scala.math.Fractional<T> extends scala.math.Numeric<T>
//!
//! Numeric$IntIsIntegral$        implements Numeric$IntIsIntegral,        Ordering$IntOrdering
//! Numeric$LongIsIntegral$       implements Numeric$LongIsIntegral,       Ordering$LongOrdering
//! Numeric$ByteIsIntegral$       implements Numeric$ByteIsIntegral,       Ordering$ByteOrdering
//! Numeric$ShortIsIntegral$      implements Numeric$ShortIsIntegral,      Ordering$ShortOrdering
//! Numeric$CharIsIntegral$       implements Numeric$CharIsIntegral,       Ordering$CharOrdering
//! Numeric$BigIntIsIntegral$     implements Numeric$BigIntIsIntegral,     Ordering$BigIntOrdering
//! Numeric$DoubleIsFractional$   implements Numeric$DoubleIsFractional,   Ordering$Double$IeeeOrdering
//! Numeric$FloatIsFractional$    implements Numeric$FloatIsFractional,    Ordering$Float$IeeeOrdering
//! Numeric$BigDecimalIsFractional$ implements Numeric$BigDecimalIsFractional, Ordering$BigDecimalOrdering
//!
//! interface Numeric$IntIsIntegral          extends Integral<Object>
//! interface Numeric$CharIsIntegral         extends Integral<Object>
//! interface Numeric$BigIntIsIntegral       extends Integral<BigInt>
//! interface Numeric$DoubleIsFractional     extends Fractional<Object>
//! interface Numeric$FloatIsFractional      extends Fractional<Object>
//! interface Numeric$BigDecimalIsFractional extends Numeric$BigDecimalIsConflicted, Fractional<BigDecimal>
//! ```
//!
//! Which one is actually picked as the implicit was confirmed by having real scalac
//! print `implicitly[…].getClass.getName` (`BigDecimal` also has a
//! `BigDecimalAsIfIntegral`, but it is not implicit).
//!
//! # Why this adds no ambiguity
//!
//! The implicit scope of `Ordering[Int]` (SLS 7.2) is `Ordering` and its parents plus
//! `Int`'s companion; `Numeric`'s companion is **not** in it. So giving
//! `IntIsIntegral` an `Integral[Int]` (`<: Ordering[Int]`) leaves the candidates for
//! `Ordering[Int]` at just `Ordering.Int`, unchanged. Real scalac also returns
//! `Ordering$Int$` for `implicitly[Ordering[Int]]`.

use crate::symbol::{SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

pub(crate) fn install(st: &mut SymbolTable, library_abi: bool) {
    let Some(ordering) = crate::classpath::find_by_jvm(st, "scala/math/Ordering") else {
        return;
    };
    let Some(numeric) = crate::classpath::find_by_jvm(st, "scala/math/Numeric") else {
        return;
    };
    add_parent(st, numeric, ordering);
    if !library_abi {
        // The private runtime (`--no-scala-library`) has neither a
        // `scala/math/Integral` classfile nor `Numeric$IntIsIntegral$`. Growing just
        // the types here would make "bytecode referring to a class that cannot be
        // loaded", so leave it alone. Unimplemented stays a diagnostic
        // (`.agent-brief.md`, "no stubs").
        return;
    }
    let integral = ensure_typeclass(st, "scala/math/Integral", "Integral");
    let fractional = ensure_typeclass(st, "scala/math/Fractional", "Fractional");
    add_parent(st, integral, numeric);
    add_parent(st, fractional, numeric);
    retype_numeric_instances(st, numeric, integral, fractional);
    add_ordering_option(st, ordering);
}

/// `object Ordering`'s `implicit def Option[T](implicit ord: Ordering[T]):
/// Ordering[Option[T]]`.
///
/// jar: `Ordering$.Option:(Lscala/math/Ordering;)Lscala/math/Ordering;`
/// (`javap -p -s scala.math.Ordering$`). The same shape of hole as `Ordering.TupleN`
/// (`prelude_ordtuple.rs`); without it `List(Some(2), None).sorted` comes out as
/// `no implicit`.
fn add_ordering_option(st: &mut SymbolTable, ordering: SymbolId) {
    let Some(module) = crate::classpath::find_by_jvm(st, "scala/math/Ordering$") else {
        return;
    };
    let mcls = st.module_class_of(module);
    if !st.lookup_member(mcls, "Option").is_empty() {
        return;
    }
    let option = st.option_sym;
    if option.is_none() {
        return;
    }
    let m = crate::prelude::prelude_method(
        st,
        mcls,
        "Option",
        vec![],
        Type::Any,
        crate::symbol::Intrinsic::None,
    );
    st.get_mut(m).flags = st.get(m).flags.with(Flags::IMPLICIT);
    let t = crate::prelude::type_param(st, m, "T");
    st.get_mut(m).tparams = vec![t];
    let param_ty = Type::Class {
        sym: ordering,
        args: vec![Type::TypeParam(t)],
    };
    st.get_mut(m).ty = Type::Method {
        paramss: vec![vec![param_ty.clone()]],
        ret: Box::new(Type::Class {
            sym: ordering,
            args: vec![Type::Class {
                sym: option,
                args: vec![Type::TypeParam(t)],
            }],
        }),
    };
    // `def Option[T](implicit ord: Ordering[T])` -- the clause is *implicit*,
    // and saying so is not decoration: a one-parameter implicit method with an
    // explicit clause is a view (SLS 7.3), and the conversion search was
    // reading this one as `Ordering[T] => Ordering[Option[T]]`. That silently
    // accepted `val o: Ordering[Option[Int]] = Ordering.Int`, which real
    // scalac rejects, and it rewrote the receiver of every failed selection on
    // an `Ordering`: `Ordering.Int` reported "value Int is not a member of
    // Ordering[Option[AnyRef]]". Only the parameter *symbols* carry the
    // implicit flag, so a method type alone cannot be told apart.
    let p = st.alloc(
        "ord",
        m,
        SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(p).ty = param_ty;
    st.get_mut(m).params = vec![p];
    st.get_mut(m).paramss = vec![vec![p]];
    if !st.get(module).members.contains(&m) {
        st.get_mut(module).members.push(m);
    }
}

/// `child[T] extends parent[T]`, passing `child`'s first type parameter straight
/// through (`Numeric[T] <: Ordering[T]`). Does nothing if the parent is already there.
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

/// Provide `trait <name>[T]` in the prelude and make it reachable by its unqualified
/// name, the way `add_scala_aliases` does.
///
/// The type parameter is named `T` to line up with the pickle side: `pickle_supply`
/// builds its scope from the **names** in `st.get(cls).tparams` in order to mirror
/// `quot(T, T): T`, so a different name cannot be mirrored. The real library's
/// `trait Integral[T]` / `trait Fractional[T]` use `T` as well.
fn ensure_typeclass(st: &mut SymbolTable, jvm: &str, name: &str) -> SymbolId {
    let id = match crate::classpath::find_by_jvm(st, jvm) {
        Some(id) => id,
        None => {
            let (pkg, simple) = jvm.rsplit_once('/').unwrap_or(("", jvm));
            let owner = crate::classpath::ensure_package(st, pkg);
            let id = st.alloc(
                simple,
                owner,
                SymKind::Class,
                Flags::INTERFACE.with(Flags::ABSTRACT).with(Flags::TRAIT),
                jvm,
            );
            st.get_mut(id).parents = vec![Type::AnyRef];
            st.get_mut(id).ty = Type::Class {
                sym: id,
                args: vec![],
            };
            id
        }
    };
    if st.get(id).tparams.is_empty() {
        let t = st.alloc("T", id, SymKind::TypeParam, Flags::EMPTY, "");
        st.get_mut(t).ty = Type::TypeParam(t);
        st.get_mut(id).tparams = vec![t];
    }
    let already = st
        .lookup(name)
        .into_iter()
        .any(|s| s == id && st.get(s).kind == SymKind::Class);
    if !already {
        st.enter_in_current(name, id);
    }
    id
}

/// Re-type `object Numeric`'s implicit instances to the `Integral[…]` /
/// `Fractional[…]` the real ABI states, and add the missing ones.
fn retype_numeric_instances(
    st: &mut SymbolTable,
    numeric: SymbolId,
    integral: SymbolId,
    fractional: SymbolId,
) {
    let Some(num_mod) = st.companion_module(numeric) else {
        return;
    };
    let num_cls = st.module_class_of(num_mod);
    let big_int = crate::classpath::find_by_jvm(st, "scala/math/BigInt")
        .map(|sym| Type::Class { sym, args: vec![] });
    let big_dec = crate::classpath::find_by_jvm(st, "scala/math/BigDecimal")
        .map(|sym| Type::Class { sym, args: vec![] });
    let table: Vec<(&str, &str, SymbolId, Option<Type>)> = vec![
        (
            "IntIsIntegral",
            "scala/math/Numeric$IntIsIntegral$",
            integral,
            Some(Type::Int),
        ),
        (
            "LongIsIntegral",
            "scala/math/Numeric$LongIsIntegral$",
            integral,
            Some(Type::Long),
        ),
        (
            "ByteIsIntegral",
            "scala/math/Numeric$ByteIsIntegral$",
            integral,
            Some(Type::Byte),
        ),
        (
            "ShortIsIntegral",
            "scala/math/Numeric$ShortIsIntegral$",
            integral,
            Some(Type::Short),
        ),
        (
            "CharIsIntegral",
            "scala/math/Numeric$CharIsIntegral$",
            integral,
            Some(Type::Char),
        ),
        (
            "BigIntIsIntegral",
            "scala/math/Numeric$BigIntIsIntegral$",
            integral,
            big_int,
        ),
        (
            "DoubleIsFractional",
            "scala/math/Numeric$DoubleIsFractional$",
            fractional,
            Some(Type::Double),
        ),
        (
            "FloatIsFractional",
            "scala/math/Numeric$FloatIsFractional$",
            fractional,
            Some(Type::Float),
        ),
        (
            "BigDecimalIsFractional",
            "scala/math/Numeric$BigDecimalIsFractional$",
            fractional,
            big_dec,
        ),
    ];
    for (name, jvm, tc, arg) in table {
        let Some(arg) = arg else { continue };
        set_instance(st, num_cls, name, jvm, tc, arg);
    }
    // Give the module side the same members, so that a qualified name such as
    // `Numeric.IntIsIntegral` resolves too (same treatment as `prelude_seq::add_numeric`).
    let known: Vec<SymbolId> = st.get(num_mod).members.clone();
    for m in st.get(num_cls).members.clone() {
        if !known.contains(&m) {
            st.get_mut(num_mod).members.push(m);
        }
    }
}

/// Create `implicit object <name> extends <tc>[<arg>]` (`<jvm>.MODULE$`), or re-type
/// the existing one.
fn set_instance(
    st: &mut SymbolTable,
    num_cls: SymbolId,
    name: &str,
    jvm: &str,
    tc: SymbolId,
    arg: Type,
) {
    let ty = Type::Class {
        sym: tc,
        args: vec![arg],
    };
    // `add_implicit_instance` **overwrites** the module symbol's `ty` from `ModuleRef`
    // to `Numeric[Int]`, so `module_class_of` can no longer reach the module class.
    // Look it up by name.
    let members = st.get(num_cls).members.clone();
    let cls_name = format!("{name}$");
    let cls = match members
        .iter()
        .copied()
        .find(|s| st.get(*s).kind == SymKind::ModuleClass && st.get(*s).name == cls_name)
    {
        Some(c) => c,
        None => {
            let c = st.alloc(
                cls_name,
                num_cls,
                SymKind::ModuleClass,
                Flags::MODULE.with(Flags::FINAL),
                jvm,
            );
            st.get_mut(c).ty = Type::ModuleRef(c);
            c
        }
    };
    let m = match members
        .iter()
        .copied()
        .find(|s| st.get(*s).kind == SymKind::Module && st.get(*s).name == name)
    {
        Some(m) => m,
        None => st.alloc(name, num_cls, SymKind::Module, Flags::MODULE, jvm),
    };
    st.get_mut(m).flags = st.get(m).flags.with(Flags::IMPLICIT);
    st.get_mut(m).ty = ty.clone();
    st.get_mut(cls).parents = vec![ty];
}
