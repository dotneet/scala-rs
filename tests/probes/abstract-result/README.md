# Abstract implementation result inference

Baseline main a900fb05, compiler 172a6525. No baseline remeasurement.
Probe.scala compiles on the saved accepted binary, then throws ClassCastException
from Raw to Wrapped at Child.value. Real scalac 2.13.16 compiles and executes it,
printing 8. Logs are `/tmp/scala-rs-abstract-result/{before,ours,nsc}.*`.

The inherited result helper was gated on a written override modifier. An
implementation of an abstract method need not write one. The candidate also
uses known abstract results in that case, without forcing pending source
signatures. Matching checks parameter sections and exact parameter types for
these abstract implementations; unrelated overloads must not inherit the result.
Inherited method type parameters are renamed to the implementing method's ones.

The first candidate fixed the cast but made Child.value return Any when Base
said Any and the body returned String. NarrowResult.scala is accepted by nsc
and rejected by that candidate; this disproved the assumption that the inherited
result should remain the final method type. The final candidate uses it as the
body's expected type, then infers the actual adapted result. Unannotated methods
remain lazy, and body completion locks allow recursive use of a known inherited
result. NarrowForward.scala reverses declaration order and also runs correctly.

The absresult fixtures cover source parents and scalac-produced binary parents,
generic classes, renamed method type parameters, unrelated overloads, narrower
results, forward references and recursive implementations. Expected stdout is
8/7/11/13/ok/done, one item per line. Both compilers reject an incompatible Int
implementation of the Wrapped result. The saved pre-fix binary rejects the
fixture's recursive method; fixture-before.compile records it. The original
minimal probe separately preserves the pre-fix runtime failure.

The unannotated Slick Join.scala probe now compiles and executes under JVM
verification with 139 SQL bytes identical to scalac. Logs:
`/tmp/scala-rs-abstract-result/slick-final.{compile,stdout,stderr}`. This is the
query probe only, not a full gitbucket or cats compilation claim. Full merge-gate
validation is pending.


Focused validation: absresult 1/1, the nine mandatory member-supply boundary
suites 534/534, and override/lazysig2/lazysig_impl2/tail1/tail3 52/52 pass
(587 total). Formatting passes. Clippy exits zero with 59 existing warnings and
no additions against the saved singleton-clippy log. The final Slick rerun still
matches scalac; no full gate has been run for this candidate yet.
