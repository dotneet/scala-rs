# Parent corpus regression repairs

The d2efbcbc full runner completed with 2278 workspace tests passing but five
corpus pass losses against e12cbdb2. All five were independently reproduced
with b0f75ad7 and compared to fresh Scala 2.13.16 compilations. No merge is
justified until these losses and the broader diagnostics are resolved.

## Applied abstract pattern bounds

pos/t10272 and pos/t12077 are valid patterns on applied abstract type members.
The compatibility check followed bounds only for bare TypeMember/TypeParam,
so Foo[A] <: A was compared as the opaque application rather than its bound.
It now substitutes the actual arguments into an applied abstract member's
upper bound before testing overlap, with the existing cycle guard.

Both corpus cases now compile as nsc does. New abstract_pattern_bounds tests
also ensure Foo[String] is incompatible with Int. Together with seqpat and
function_pattern, 22 focused tests pass. Evidence:
/tmp/scala-rs-codex/integration/corpus-regressions-parent/latest-before.json,
bounds-after.json and bounds-focused.log. Temurin 17 and UTF-8 were pinned.
The new tests are accept/reject checks, not runtime evidence.

Remaining losses:
- pos/t6275: valid existential type binder B[t] tested against A[Int] rejected.
- neg/t10073 and t10073b: unresolved implicit conversion type parameter Unused
  incorrectly receives ClassTag evidence. scalac reports unresolved spliceable
  type. Inspect classtag_erasable and inference finalization: not being in the
  lexical scope is currently treated as enough to materialize a type parameter.
  Preserve valid empty-array/varargs inference cases when repairing it.

Full workspace/corpus and library gates for the repaired candidate are pending.
