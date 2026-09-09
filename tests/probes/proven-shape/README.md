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
