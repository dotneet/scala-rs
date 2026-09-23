# What the hand-written prelude drops that the pickle carries

`crates/typer/src/prelude*.rs` declares thousands of library symbols by hand.
A member `PickleSupply` installs from a `ScalaSignature` arrives with the
flags nsc pickled, a parameter symbol per parameter and its constructor
fields; a member `prelude::class` / `prelude::method` builds arrives with
`Flags::FINAL` and little else.

That would not matter if the pickle won. It does not: member lookup finds the
hand-written member first, and `Check::supply_from_pickle` runs only when
nothing matched, or when the class file declares a signature none of the
candidates has (`Check::supply_receiver_override`). So an attribute the
prelude leaves off is an attribute the compiler does not have — in
`--scala-library` mode as much as in the private-runtime one.

A survey compared every prelude class under `scala/` with the same class's
`ClassSig` from the 2.13.16 jar (loaded through `SigLoader`): `CASE`,
`SEALED`, `ABSTRACT`, `TRAIT` / `INTERFACE`, `FINAL`, type-parameter count,
and per member `IMPLICIT`, parameter-symbol count, by-name and defaulted
parameters. The survey is a throwaway `#[cfg(test)]` module that prints a
list; it is not kept in the tree. Its findings follow.

## Fixed

- `Some`, `Left`, `Right`, `Success`, `Failure` carry `CASE`, so
  `Some(1).copy(value = 2)` works.
- `::` is one class symbol (it used to have an empty twin beside
  `$colon$colon`), so `val c: ::[Int]` and `new ::(1, Nil)` work.
- A qualified constructor pattern (`case Ior.Left(a)`) resolves its class
  through the qualifier, not by looking the last segment up lexically.
- Prelude methods have parameter symbols with the library's names, so named
  arguments work on them (`List(1, 2, 3).mkString(sep = "-")`).
- Lower-bounded members (`prelude_lowbound.rs`): `List.contains`,
  `indexOf`, `reduce`, `reduceLeft`, `reduceRight`, `toArray`, `sorted`,
  `min`, `max`, `sum`, `product`, `Map.+`, `Map.updated`,
  `Option.getOrElse` / `orElse` take `[B >: A]` as the library does, with
  unchanged erasure (`crates/cli/tests/preludelb.rs` checks the call sites
  with `javap -c`). `Map` key parameters are typed `K`.
- `Ordering <: PartialOrdering <: Equiv` and `object Equiv`'s instances
  (`prelude_eqtail.rs`); `Ordering#compare` is `(T, T): Int`.
- Erasure bridges. The prelude stamps `FINAL` on every method it declares,
  abstract ones included, so the bridge passes trust `FINAL` only where
  modifiers are real (symbols from this run's sources or from a pickle, as
  `override_check::modifiers_are_known` does) and never on a deferred
  member. And because the prelude declares `Numeric`, `Fractional` and
  `Integral` without members, a source class that extends a generic `scala.*`
  ancestor has the overridden members completed from the pickle before
  bridging, so `class Num extends Numeric[Int]` gets `fromInt(I)Object`
  (`crates/cli/tests/library_trait_bridges.rs`).

## Still wrong

- **`SEALED` missing** on `Option`, `List`, `Either`, `Try`, `Vector`,
  `Range`, `NumericRange`, `<:<`, `=:=`, `immutable.BitSet`,
  `immutable.Queue`, `mutable.ArraySeq`, `mutable.PriorityQueue`,
  `mutable.TreeMap`, `mutable.TreeSet`. `new Option[Int] { … }` is accepted
  (scalac: "illegal inheritance from sealed class Option") and, if it
  overrides one of the jar's final methods, fails at class-load time with
  `IncompatibleClassChangeError`. A match over these types draws no
  exhaustivity warning; that also needs `Symbol::children` for the prelude
  hierarchies.
- **`ABSTRACT` missing** on `Option`, `List`, `Either`, `Try`, `Vector`,
  `Range`, `collection.Seq`, `immutable.BitSet`, `mutable.ArraySeq`,
  `collection.WithFilter`, and wrongly set on the annotation classes
  `inline`, `noinline`, `volatile`, `transient`, `native`, `switch` and
  `uncheckedVariance`.
- **`FINAL` on classes the library does not declare final**, because
  `prelude::class` sets it unconditionally. Mostly inert
  (`modifiers_are_known` excludes prelude symbols from "cannot override final
  member"), but `Check::is_final_like` reads it when deciding whether a
  stable-identifier pattern's type and the scrutinee can be inhabited
  together (`Check::stable_pattern_compatible`).
- **`collection.Seq` is not marked `TRAIT`** (probed harmless), and
  **`runtime.LazyRef` declares no type parameter**.
- **Missing `DEFAULTPARAM`** on `ArrayOps.indexOf`, `ArrayOps.lastIndexOf`
  and `ArrayOps.indexWhere`, and on the constructors of
  `mutable.ArrayDeque`, `Queue` and `Stack`. The three `ArrayOps` members
  model the default as a shorter overload that codegen completes, so both
  spellings compile. `List`
  declares `indexWhere` and `startsWith` in both arities the same way
  (`crates/cli/tests/seq_default_args.rs`).
- **`ArrayOps.mkString`**: the prelude declares it on `ArrayOps`, the library
  reaches `IterableOnceOps.mkString` through `genericWrapArray`, so
  `Array(1, 2, 3).mkString(sep = "|")` reports "named arguments (method
  parameters not resolved)".
- **Nested classes spelled with `$`** (`scala/Predef$ArrowAssoc`,
  `scala/util/Either$LeftProjection`, `scala/Option$WithFilter`) have no
  pickle under the name their JVM name implies. `PickleSupply::complete_named`
  has its own nested-spelling retry, so this is not necessarily a defect.
- No member the survey could match was missing `IMPLICIT`, and none dropped a
  by-name parameter.
