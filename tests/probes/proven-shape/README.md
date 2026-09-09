# ProvenShape conversion investigation

Baseline: accepted main 0afdfd22 (compiler tree 88ab9308). No baseline
remeasurement. Experiments use the accepted shape-join release binary, real
scalac 2.13.16, the published Slick dependencies in
`/tmp/scala-rs-shape-join/cp`, and the real Scala library jar.

Implicit.scala is rejected at `def *: ProvenShape[Int] = id` with Rep[Int]
found instead of ProvenShape[Int]. Explicit.scala and Inferred.scala call
ProvenShape.proveShapeOf directly, with and without explicit type arguments.
Both compile with both compilers. All four executions exit zero under
java -Xverify:all and produce byte-identical SQL:

```
select x2."ID", (case when (x3."ID" is not null) then x3."ID" else null end) from "ROWS" x2 left outer join "ROWS" x3 on x2."ID" = x3."ID"
```

Runtime classpath must include scala-library explicitly (the dependency cp
file omits it). Logback uses `/tmp/scala-rs-shape-join/quiet.xml`.
Full compile/run logs are in `/tmp/scala-rs-proven-shape`.

The hypothesis that this example cannot solve Shape at all is disproven by
both explicit calls. Imported.scala still fails after importing proveShapeOf;
loading the companion in an earlier object also does not fix it. Small.scala,
a two-parameter conversion with a Shape[A,B] witness, is accepted and executes
as value=7 with both compilers. It is a control, not a regression reproducer.

A temporary trace in conversion_provides (removed after observation) captured
trace.txt on Imported.scala. The actual conversion candidate is found. Its
first parameter T becomes Rep[Int], but U stays the conversion's TypeParam.
The wanted type is ProvenShape[Int]. open_conversion_fit unifies its result
with that wanted type, then calls conv_implicits_resolve(id, from), which
recomputes arguments from the source alone. The next hypothesis is that the
result-derived U=Int is discarded before checking the Shape witness. This is
not yet established as the only cause; binary implicit-scope loading must also
be checked. No compiler correction or new full gate is claimed in this record.


## Correction: missing completion, not result inference

Passing result-derived U=Int into open_conversion_fit's witness search still
fails: target-trace.txt records the correctly substituted Shape request and
found=None. WarmWitness.scala adds an earlier explicitly requested Shape; the
accepted baseline then compiles the implicit conversion without any compiler
changes. Thus the previous result-inference hypothesis is not the cause of
this Slick failure. That experimental compiler change was removed.

The candidate instead retries a failed value conversion after loading applicable
conversions' witness scopes and candidate inheritance, as normal implicit
application already does. The wanted result companion is loaded too. The
unmodified Implicit.scala now compiles and executes under java -Xverify:all,
producing the same SQL bytes as the scalac executions. Logs:
`/tmp/scala-rs-proven-shape/implicit-warm.{log,stdout,stderr}`.

The permanent proven_lib.scala fixture is compiled with real scalac and has a
bounded Shape level in an existential witness. The saved accepted compiler
rejects proven.scala with Rep[Int] required Proven[Int], while the candidate
and nsc accept. The simple source-only control did not expose unloaded binary
parents. `proven` compares runtime output and requires both compilers to reject
Proven[String] from Rep[Int]. Full merge-gate validation is still pending.


## The negative probe exposes a second fault

Witness completion alone makes the positive Slick and small binary cases run,
but the permanent negative probe is then incorrectly accepted: Proven[String]
from Rep[Int]. Both the accepted baseline and scalac reject it. The previous
claim about result inference therefore needs qualification: it is not sufficient
to fix the unloaded witness, but after completion it is required for soundness.
The open conversion path solved the result as String and then recomputed a
Shape[Rep[Int], Int] from the source, accepting unrelated evidence.

The corrected candidate checks implicit clauses using argument- and result-
derived substitutions together, retaining source-inferred arguments for any
parameters not determined by that unification. Logs for the rejected completion-
only candidate are `/tmp/scala-rs-proven-shape/bad-{before,candidate,nsc}.log`.
This is a real acceptance regression found before a full gate, not an accepted
intermediate implementation. Both corrections must survive the runtime and
rejection tests together.


Corrected focused validation: proven 1/1 plus all nine mandatory member-supply
boundary suites 534/534 pass. The unchanged Slick Implicit.scala probe compiles
and executes with exit zero; its 139 stdout bytes match scalac exactly after
the soundness correction. Logs: implicit-corrected.{log,stdout,stderr} and
boundary-corrected.log under `/tmp/scala-rs-proven-shape`. No baseline counts
have been remeasured, and full-gate acceptance remains pending.
