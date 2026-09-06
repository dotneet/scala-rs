# Compiler-defined Singleton

Strict pattern type-name lookup exposed the missing scala.Singleton marker in
pos/t10569. Scala 2.13.16 accepts that source, while scala-library has no
scala/Singleton JVM class.

The prelude now installs a distinct nominal marker with Any as its parent.
The marker itself is neither AnyRef nor AnyVal. Constants other than Unit,
stable paths, module references and this types can
conform to it. A fresh Object, mutable variable and ordinary method result do
not gain singleton conformance. Existing stable-path resolution is reused.
The marker erases to Object. For a receiver typed Any/AnyVal the Singleton test
is total, including a null passed as Any; for a statically reference-typed
receiver it checks non-null. The type checker preserves the total-pattern
marker, and isInstanceOf makes the corresponding choice after erasure while
still evaluating its qualifier. Static-tests/results.json records scalac's
contrasting Any, AnyRef, Null, Singleton and primitive cases. This distinction
was discovered by the side-effect fixture: its block has type Null, and nsc
prints false after evaluating the effect.


Method inference retains a constant type when the type parameter is bounded by
Singleton. Ordinary inference still widens constants. The unification helper
now separates precise extraction from the existing widening wrapper.

Oracle fixtures cover direct and qualified names, alias and compound types,
constants, stable selections and parameters, generic bounds, invalid mutable
and fresh arguments, the original tuple pattern, and strict JVM output.
An additional probe found that scalac rejects Unit as Singleton; the constant
conformance arm explicitly excludes it.

Evidence is under /tmp/scala-rs-codex/integration/singleton-parent. Initial seven
oracle cases passed. The extended generic-bound test initially failed because
inference widened 1 to Int; bound-inference.log records the repaired pass.
final-focused.log passed the four related integration tests, including 17
Singleton accept/reject fixtures and strict JVM pattern/generic output. Bounded
library measurements match aa5d5e0b: cats 346/81, gitbucket 899/112, Slick
0 errors/1490 classes, scala library 1558/169. These measurements precede the
isInstanceOf emission followup; instanceof-focused.log must be checked for its
additional null and side-effect runtime checks. That first followup failed
against nsc because the expected static-Null result was wrong. static-repaired.log
passed with the correction. The static-tests/ours-results.json strict JVM
output matches scalac for all seven static-type controls. Those controls are
also retained in static_runtime.scala; final-oracle.log passed all 18 fixtures
and both strict JVM output comparisons.
Do not count the failed followup as validation.

Unchecked/fruitless-type-test warnings are not implemented by this change.
Full workspace, corpus and interoperability acceptance is still required;
this note does not claim main acceptance or complete Singleton support.

The e0c92b19 full corpus audit completed with three regressions: t10569,
pos/t11174b and run/t2788. The latter two still reproduce in this worktree
(new-losses/results.json): an escaping conversion parameter stays rigid in
from.foo(23), and Array[Option[Int]].flatten cannot find its view evidence.
These are separate acceptance blockers; do not start another full acceptance
run before repairing them, and do not merge this accumulated candidate yet.
