# Pattern acceptance audit

The d2efbcbc/2d223581 corpus ledgers count neg/t7984 and neg/t8597 as gains,
but both are fatal-warning tests. Without -Xfatal-warnings, Scala 2.13.16 accepts
them. Our compiler rejected valid patterns for unrelated reasons instead.

List[Any]("s") was initially typed as List[Any], then the factory result heuristic
replaced its argument with String. Explicit factory type arguments now prevent
this inferred result reconstruction. Some had a separate problem: its prelude
apply was not polymorphic and a bespoke result rewrite supplied the element
type. Some.apply now has [A](A): Some[A], and that rewrite has been removed.

The prelude's class helper also marks classes final by default, including List.
Typed pattern compatibility now reads both variance and actual finality from
ScalaSignature, preserving source-defined classes. Provisional FINAL flags no
longer reject List[C] against Seq[D] where D extends C.

Validation under Temurin 17 and UTF-8:
- 26 related tests pass (some-focused.log), including seven accept/reject oracle
  fixtures for the original programs without fatal warnings, Seq/List subtyping,
  and incorrect String assignments from explicitly widened List/Some/Map/Vector.
- Six previous positive corpus gains freshly compile with both compilers:
  arrays2, simplelists, t3577, t5259, tcpoly_seq, tcpoly_seq_typealias.
- Bounded measurements: cats 346 errors/81 files; gitbucket 899/112;
  Slick 0 errors/1490 classes; scala library 1558/169. Individual times were
  26.07, 14.80, 4.27 and 7.27 seconds respectively with other validation active.

Compared with e0c92b19's bounded measurements, four new Some[A] diagnostics are
cascades at already failing arguments, not newly rejected standalone programs:
Unit=>B is mistaken for Function0 in cats; overloaded Stream.tail fails Tuple2
construction at two sites; and Regex's String.toList selection fails. The same
argument errors exist in the earlier log. Reduced standalone tuple/if/string
examples compile; the Unit example reproduces both the primary and cascade
error. These underlying defects and diagnostic noise are not claimed fixed.

Evidence: /tmp/scala-rs-codex/integration/corpus-gain-audit, including
patterns-results.json, positive-results.json, some-focused.log,
explicit-repaired.json, some-context-results.json, and measures/diagnostic-delta.json.

Unchecked pattern warnings remain unimplemented. The two fatal-warning negative
cases may return to their baseline failing status; do not retain a false type
error to make their corpus status green. Full validation of this combined repair
is pending. The completed 2d223581 audit also found pos/t10569's missing Singleton
built-in; repair it before integration. No main merge is justified yet.
