# Member and application batch, based on b4431153

Inventory before implementation. Each diagnosis is a hypothesis; correcting it
from measurements is more valuable than implementing the proposed mechanism.
The accepted baseline is 647afcf2; do not rerun aggregate before measurements.
Evidence: /tmp/scala-rs-next-batch-probes/inventory/results.json and adjacent
compiler/runtime logs; the before compiler was rebuilt at 647afcf2.

| Candidate | Measured evidence | Mechanism / scope decision |
|---|---|---|
| Upper-bounded wildcard members | Valid List[_ <: Base] member lookup rejected; nsc runs | Include: use upper bound for member lookup; inherited generic result and erased receiver need runtime coverage |
| Curried implicit extension | Valid lambda rejected; invalid Int accepted and VerifyError | Include: fallback returns final result without ordinary remaining-clause handling |
| Option.flatten | Generic nested Option rejected; Option[Int] accepted and ClassCastException | Include: real polymorphic <:< evidence; remove fabricated backend witness |
| Option.zip | Valid Option[(Int,String)] rejected | Include: preserve independent second element parameter |
| Option.collect | Initial nsc call is ill-inferred too; explicit type arguments needed | Reprobe: declaration uses Any instead of independent result parameter |
| Try.flatMap | Generic A -> Try[B] rejected | Include: independent result parameter |
| Try.transform | Int -> Try[String] rejected | Include: independent result parameter |
| Try.orElse | Valid widening fallback rejected | Include: result lower bound and by-name argument |
| Try.recover/recoverWith/collect | recover[Any] simple runtime passes despite approximate declaration | Probe neighbors bidirectionally while repairing the same declaration family |
| Array.toMap | Generic Array[(A,B)] rejected | Include: real evidence plus wrapped IterableOnceOps runtime invocation |
| Untyped catch binding | Inferred Any rejects Throwable annotation; nsc runs | Include: type catch patterns against Throwable; extractor and guard coverage |
| Vector grouped/scanLeft/sorted | All isolated probes pass and execute identically | Initial missing-signature hypothesis unconfirmed; investigate loading context separately |
| SortedSet.filter | Isolated probe passes and executes identically | Same: do not overwrite signatures from the diagnostic alone |
| Method tupled | Both compilers reject bare def f.tupled | Reject this reduction; actual case-companion use needs separate probe |
| Manifest | Real scalac compiles and runs, scala-rs missing type/evidence | Broader reflection evidence generation; separate implementation investigation |
| Written class bounds | Parent/annotation/alias B[String] accepted for A <: Number; constructors reject | Broader completion-order-sensitive type validation; separate hard root |
| Slick Shape / OptionLift | Remaining diagnostics in accepted full run | Higher-kinded implicit evidence chain; no new reduced root established yet |
| Gitbucket macro source-class tags | Numerous mapTo expansion failures | Existing source type-tag bridge limitation; separate hard root |

Implement the supported member/application repairs as one batch, then focused
new and historical regressions, environment preflight, and one composed full
gate. Retain malformed/negative inputs independently so one rejection cannot
hide acceptance of another. Keep full-run-only candidates open, not "fixed"
because an isolated reduction passes.


First composed probes: both normal programs compile and execute with byte-exact
scalac output. Independent negatives exposed another shared root: an unresolved
implicit-only method in an inferred val/def can bypass adapt (NoType expected)
and be eta-expanded later. The existing known-expected-type backstop now also
runs at inferred definition boundaries. This is required to reject both
Option[Int].flatten and Array[Int].toMap, not a name-specific special case.


Before the full gate: 616 related tests passed (23 suites); the receiver-type
follow-up passed 28 tests in five affected suites. Final Option.zip uses the
released two-parameter signature, including A1 >: A, rather than a one-parameter
approximation. The final signature/negative probes, Either/Try regressions and
conformance suite passed together. The 18 historical corpus identities have
zero changes and losses. Final clippy retains 57 warnings with no additions or
removals. Preflight validates four pinned source trees, 121 jars, 33 Java support
classes and 1498 scalac reference classes. Final before probes are retained in
/tmp/scala-rs-next-batch-probes/before-final/results.json.
