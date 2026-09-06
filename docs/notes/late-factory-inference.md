# Late classpath factory inference — parent candidate

Based on bc8e87f4; not yet accepted into main.

`scala.collection.immutable.Stream.apply(a)` inside `def pure[A](a: A)`
returned `Stream[A]` containing the *factory method* A, rather than the caller
A. Explicit `.apply[A](a)` worked. Raw type tracing demonstrated equal Stream
class symbols but distinct type-parameter symbols. After an initial overload
failure, `widen_module_from_pickle` loaded the real polymorphic method and its
success path returned the declared result without normal inference.

Both companion-widening success paths now rejoin the ordinary selected-call
pipeline. This preserves the existing fallback ordering while applying type
inference, bounds, defaults and implicit clauses in the same place as ordinary
calls. A loop encloses the existing resolution match; most of the diff is its
required indentation, not changed inference logic. No tracing remains in source.

Validation (Temurin 17, UTF-8, Cargo and test jobs 2):

- late_factory_inference, lazysig_impl2, ordsummon, ovl_exptype: 28 passed,
  0 failed. New fixture compares nsc and scala-rs with strict JVM verification
  on generic Int/String results, and requires rejection of a generic result
  falsely annotated Stream[Int].
- Against bc8e87f4: cats 351/81 -> 350/81; exactly the Stream[A] diagnostic
  removed and no added diagnostic. GitBucket 902/112 unchanged; Slick 0 errors
  and 1490 classes unchanged; Scala library 1856/203 unchanged.
- Evidence: /tmp/scala-rs-codex/integration/stream-prepend-probe/,
  focused.log, repaired-results.json, trace2.log and measures/.
- Full workspace/corpus/Slick execution verification for this change is pending.
  The parent chain still differs from accepted main and must not be merged on
  these focused results alone.

Unresolved: unqualified `Stream(a)` still reports no matching overload in the
minimal pure.scala probe. Placing an explicit qualified Stream.apply[Int](0)
use before that declaration instead exposes kind/type-argument errors on the
unqualified Stream type (warm.scala / warm-results.json). This proves a separate
completion-order issue, not that the current change implements all factory
sugar. Keep these probes when investigating module and class alias identities.
