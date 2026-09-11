# Next batch inventory: collection extension, builder, and sorted-set calls

This batch starts from local `main` at `0f61624f`, with the accepted metrics in
`tests/BASELINE.md`. The baseline is recorded there and is not rerun as a new
"before" measurement. The observations below come from reduced probes against
the release binary and scalac 2.13.16.

The diagnoses are hypotheses until the probes and runtime checks confirm them.
Correcting a hypothesis through measurement is the main value of this slice.

| Target | Evidence | Planned scope |
|---|---|---|
| `IterableOnce.reduceOption` extension | A source implicit class with the same extension name as the library conversion was rejected by scala-rs and accepted by scalac. The cats compat source then produced the same failure. Separating lexical candidates from companion candidates makes the reduced source compile and run. | Keep lexical implicit conversions ahead of companion conversions, while preserving ambiguity among candidates in the same scope. |
| `ReusableBuilder` parent | `Vector.newBuilder[A]` is typed as `ReusableBuilder[A, Vector[A]]`. scalac accepts it as `Builder[A, Vector[A]]`; scala-rs did not because the classfile placeholder had no pickled parent attached. | Attach the Scala-signature parent for an existing library placeholder, with the existing prelude boundary intact. |
| `SortedSet` set operators | The cats `SortedSetSemilattice.combine` call `x | y` fails with `Set[A]` versus `SortedSet[A]`, while the direct concrete fixture passes. The pickle path exposes an inherited `SetOps` result through a less-specific parent. | First add only the concrete `SortedSet` operator signatures and ABI dispatch required by the reduced cats shape; retain the existing `Set` behavior. |

The same collection hierarchy also exposed a fourth, smaller case: a Scala
library trait placeholder created for `SeqHasAsJava` was marked as a Java class.
That made `inherited_superclass` look through `immutable.IndexedSeq` to a
constructor-bearing `collection.Seq` and reject a source class extending the
trait. `prelude_hier::ensure_link` now normalizes all linked collection symbols
to interface/trait flags, with a dual-run fixture in
`tests/fixtures/collresults_indexedseq_trait.scala`.

The batch is accepted only after focused regression tests, cats kernel
measurement, source/jar/cache preflight, and one `tests/verify_merge.sh` run
with `VERDICT=PASS`, `DONE`, and zero corpus losses. The gate tree and any
handoff commit must be compared before updating `BASELINE.md` and the raw
corpus ledger. Java Object overrides and other high-complexity families remain
deferred until a smaller bidirectional probe identifies a complete repair.
