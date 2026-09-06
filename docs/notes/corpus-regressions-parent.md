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

## Case-local pattern type binders (parent repair)

Typed patterns now introduce lowercase type arguments as case-local skolems.
They inherit the declaration's kinds and bounds, and are constrained by the
scrutinee with variance respected: invariant positions can establish equality;
covariant positions only establish an upper bound. Backquoted identifiers
remain references to existing types, and unknown references are diagnosed.
The skolems remain rigid after leaving the case scope. Otherwise branch LUB
inference minimized them to Nothing and generated a String stack-map type for
an Object return, producing a VerifyError in an otherwise accepted match.

The permanent pattern_type_binders test compares 16 cases with Scala 2.13.16,
including t6275, accepted and rejected assignments, scope/shadowing, declaration
bounds, higher kinds, variance and backquoted references. Its runtime case
runs both outputs with -Xverify:all and checks 42/bound/other. Together with
abstract_pattern_bounds, function_pattern and seqpat, 23 tests pass.
Evidence: binders/quoted-focused.log and binders/rigid-runtime-results.json
under the evidence directory above.

Bounded library measurements: cats 347 errors/81 files; gitbucket 902/112;
Slick 0 errors/1490 classes; scala library 1851/202. Compared with cd214262,
the diagnostic multiset loses three Eval.scala errors and four Java collection
wrapper/TailCalls errors, with no added diagnostics. These are compilation
measurements, not proof that the complete libraries run correctly.
See binders/measures/diagnostic-delta.json.

A further probe exposed an existing missing type-application bounds check:
`def bad[T,F[X <: T]]: Any = null.asInstanceOf[F[Int]]` is wrongly accepted
by the older bc8e87f4 binary too, while scalac rejects it. Pattern uses of the
same higher kind also expose this defect. apply_types currently checks arity
but not these bounds. Do not infer complete higher-kind compatibility from
the positive binder test. Nested kind variance/owner-bound copying also needs
a broader audit. Evidence: binders/kinds/results.json and plain.scala.

The two ClassTag corpus losses remain unresolved. Full workspace/corpus and
runtime gates for this new candidate remain pending; it is not merged.
