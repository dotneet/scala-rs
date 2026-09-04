//! The equivalent of the `scala` package object's `val Ordering = scala.math.Ordering`.
//!
//! nsc's `package object scala` (`src/library/scala/package.scala`) makes the type
//! classes visible unqualified in **both the type and the term namespace**:
//!
//! ```scala
//! type Ordering[T] = scala.math.Ordering[T]
//! val  Ordering    = scala.math.Ordering
//! ```
//!
//! `prelude::add_scala_aliases` installed only the former (the `type`). All that
//! `st.enter_in_current("Ordering", <trait>)` enters is a *class* symbol, so
//! `Ordering` in **term** position resolved to that trait as well:
//!
//! - `Ordering.Int` went looking for "a member `Int` of the trait `Ordering`" and
//!   came out as `value Int is not a member of Ordering`. The proof is that fully
//!   qualifying it as `scala.math.Ordering.Int` did work.
//!   (After `agent/integral` added an implicit `Ordering.Option`, the search for an
//!   implicit conversion on a receiver with no such member picked up that
//!   `Ordering[T] => Ordering[Option[T]]` and the error text turned into
//!   `… is not a member of Ordering[Option[AnyRef]]`. Only the message changed;
//!   the cause is here.)
//! - `Ordering[String]` came out as "a type application of a trait in term
//!   position", went through typechecking **silently**, and had codegen push
//!   `Ordering$.MODULE$` and checkcast it to `Ordering`. At run time,
//!   `ClassCastException: scala.math.Ordering$ cannot be cast to scala.math.Ordering`.
//!
//! Entering the companion module into the same scope makes `SymbolTable::lookup`
//! return both the class and the module; term position (`check::type_ident`) picks
//! the module and type position (`check::resolve_type_name`) picks the class.
//!
//! The summon side (`Ordering[String]` = `Ordering.apply[String]`) is handled by the
//! module-to-`apply` redirect in `check.rs`. The jar's
//! `Ordering$.apply:(Lscala/math/Ordering;)Lscala/math/Ordering;` is supplied from
//! the pickle, so we do not hand-write the signature here (hand-writing it would
//! grow a second one beside the pickled one and make an overload).
//!
//! Under `--no-scala-library` there is neither a `scala/math/Ordering` classfile nor
//! an `Ordering$`, and `add_scala_aliases` itself enters nothing, so the diagnostic
//! stays `not found: value Ordering`. This is gated on `library_abi` too.

use scala_rs_parser::{Flags, SymbolId, Type};

use crate::symbol::{SymKind, SymbolTable};

/// Enter the companion module into the term namespace too, under the same spelling
/// as the alias `add_scala_aliases` installed as a type.
const ALIASES: [&str; 7] = [
    "scala/math/Ordering",
    "scala/math/Numeric",
    "scala/math/Equiv",
    "scala/math/Fractional",
    "scala/math/Integral",
    "scala/math/BigInt",
    "scala/math/BigDecimal",
];

pub(crate) fn install(st: &mut SymbolTable, library_abi: bool) {
    if !library_abi {
        return;
    }
    for jvm in ALIASES {
        let Some(cls) = crate::classpath::find_by_jvm(st, jvm) else {
            continue;
        };
        if st.get(cls).kind != SymKind::Class {
            continue;
        }
        let name = st.get(cls).name.clone();
        let m = match st.companion_module(cls) {
            Some(m) => m,
            // `prelude_numhier::ensure_typeclass` grows `trait Integral[T]` /
            // `trait Fractional[T]` without reading the jar, which leaves only one
            // half of the companion pair. The jar really does have
            // `scala/math/Integral$.apply:(Lscala/math/Integral;)Lscala/math/Integral;`
            // (`javap -p -s scala.math.Integral$`), so only once the module is made
            // here does `Integral[Int]` become `Integral.apply[Int]`. Without it the
            // trait itself stood in term position, `val i: Integral[Int] =
            // Integral[Int]` went through **silently**, and at run time it was
            // `ClassCastException: scala.math.Integral$ cannot be cast to
            // scala.math.Integral`.
            None => make_companion(st, cls, jvm, &name),
        };
        if st.lookup(&name).into_iter().any(|s| s == m) {
            continue;
        }
        st.enter_in_current(&name, m);
    }
}

/// Create `object <name>` (`<jvm>$`) under the same owner as the class.
fn make_companion(st: &mut SymbolTable, cls: SymbolId, jvm: &str, name: &str) -> SymbolId {
    let owner = st.get(cls).owner;
    let module_jvm = format!("{jvm}$");
    let mcls = st.alloc(
        format!("{name}$"),
        owner,
        SymKind::ModuleClass,
        Flags::MODULE.with(Flags::FINAL),
        &module_jvm,
    );
    st.get_mut(mcls).ty = Type::ModuleRef(mcls);
    st.get_mut(mcls).parents = vec![Type::AnyRef];
    let m = st.alloc(name, owner, SymKind::Module, Flags::MODULE, &module_jvm);
    st.get_mut(m).ty = Type::ModuleRef(mcls);
    m
}
