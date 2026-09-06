# ClassTag inference and materialization

The two corpus regressions neg/t10073 and t10073b pass an unconstrained type
parameter through an implicit conversion result and request ClassTag evidence
for that parameter. Scala 2.13.16 rejects the call with an unresolved spliceable
type diagnostic. conv_targs instead replaced the variable with AnyRef, allowing
our compiler to synthesize Object evidence and accept the program.

Keep variables that escape in conversion results open. Variables absent from
the result may be minimized to their lower bound, after searching the implicit
clauses for witnesses that determine them. A direct unresolved ClassTag request
at the selected conversion reports the error at the receiver use. Nested
ClassTag[List[T]] and ClassTag[Array[T]] remain materializable by erasure.

A second defect was hidden by compile-only checks: unconstrained ordinary calls
returned Object evidence where scalac returned Nothing. The prelude incorrectly
registered ClassTag's standard value getters as implicit candidates. They are
plain vals in Scala 2.13's reflect/ClassTag.scala (Int at line 105, AnyRef at 114,
Nothing at 115). They now remain ordinary getters. Materialization selects the
canonical value after knowing the requested type. This also avoids routing a
Nothing tag through a synthetic bottom-typed classOf expression, which caused
a runtime ClassCastException.

Ordinary implicit-clause inference keeps partial witness bindings. For example,
W[Int] determines T in f[T,U](Any)(W[T],ClassTag[U]); the unresolved U is then
minimized for materialization. Variables in failed ordinary evidence requests
remain open, preserving scalac's OM[Long,Long,R] diagnostic rather than reporting
OM[Long,Long,Nothing]. Ambiguity must not be hidden by minimization.

Evidence lives under /tmp/scala-rs-codex/integration/classtag-parent:
- corpus-after.json: original two regressions correctly rejected by the first
  prototype, before the later materialization changes.
- canonical-focused.log: 15 fixture acceptance/rejection comparisons and three
  strict JVM output comparisons pass after canonical selection.
- related-focused.log: 31 existing tests pass, one diagnostic regression exposed
  the overly broad minimization and was compared with fresh scalac output.
- partial-results.json: a valid partially inferred witness case rejected before
  the latest fix; scalac prints Nothing.
- partial-focused.log: all 33 tests pass, including 16 oracle fixtures and four
  strict JVM runtime comparisons inside classtag_inference.

The permanent classtag_inference test additionally includes the partial witness
runtime case, making 16 fixtures and four runtime comparisons. Full workspace,
library measurements and corpus gates are still required. This work is not
accepted or merged. More general inference constraints, dependent lower bounds
and higher-kind compatibility remain outside what these tests establish.

The first bounded measurement kept cats at 347 errors but gitbucket exceeded
120 seconds (previous binder candidate: about 16 seconds). A live CPU sample
shows nested implicit search from fill_defaults_and_implicits. The partial
inference continuation was also rerunning failed searches on calls with no
ClassTag request. A subsequent change limits that continuation to clauses that
actually contain ClassTag; gated-focused.log and perf-gated/measures are the
follow-up evidence locations. This remains a performance acceptance blocker
until measured again. Original measurements and sample are preserved under
measures/ and gitbucket-sample.txt.

The gated follow-up passes all 33 related tests. Bounded measurements completed:
cats 347 errors/81 files (1.81 s), gitbucket 899/112 (5.73 s), Slick 0 errors and
1490 classes (5.02 s), scala library 1849/202 (10.14 s). Wall times are individual
observations with another workspace runner active, not a controlled speedup
claim. The previous 120 s timeout did not recur.

The diagnostic multiset has no added error locations against the binder
candidate. Changed messages retain unresolved type parameters instead of
inventing AnyRef; the anonymous-class suffix change is only a generated name.
The five removed locations are tuple ordering conversions in gitbucket (three)
and ClassTag[Throwable] conversion requests in scala library Exception.scala
(two). Whole-program correctness for these libraries is still not established.
See perf-gated/measures/diagnostic-delta.json. Final combined full gates remain
pending; the separate 2d223581 full runner excludes these ClassTag changes.
