# Source identities and erased field access batch

This batch continues the rejected lexical-context candidate on a worktree
composed with local main 18e258a4. The accepted compiler baseline is b969b0d1;
693f8966 is a rejected intermediate candidate, not the accepted baseline.
No accepted-before aggregate was remeasured.

## Inventory and implementation

The inventory was a set of hypotheses, not diagnoses to implement blindly.
Correcting a hypothesis through measurement is an important result. The first
four rows were reproduced before implementation and repaired together. Separate
compilation exposed the last two related gaps, which were included before the
full gate. No subagents were used.

| Boundary | Reproduction and cause | Difficulty and repair |
| --- | --- | --- |
| Function source identity | Two files with the same-shaped qualified private-package call reuse a parser NodeId. Legal access is rejected; illegal access is accepted in one source order. | Medium: key function symbols by compilation unit and node, preserve transported macro symbols, assign real identities to synthetic bodies. |
| Generic var stores | `Cell[Int].value = 7` uses `I` although the declaration stores Object. Unit and bounded references also fail. | Low/medium: use the declaration's JVM field descriptor, consistent with erased RHS storage adaptation. |
| Lazy getters | `Cell[Array[Int]].value(0)` passes Object to an array instruction. | Low/medium: adapt the result after the lazy getter's early return path, including unqualified reads. |
| Unqualified abstract fields | An inherited `type T = Int` field read leaves Object for `ireturn`. | Medium: share declaration/use adaptation with qualified reads, preserving local frame slots and assignment locations. |
| Parameterless receiver application | Loaded array getters do not participate in omitted `.apply`. | Medium: extend existing result apply insertion to arrays, retaining the method identity and open type parameters. |
| Lazy field/accessor identity | A loaded lazy property's storage symbol and getter enter overload resolution as separate alternatives. | Low/medium: collapse the storage field behind its matching same-owner accessor, as for constructor fields. |

The initial access reductions using unqualified private calls did not reproduce
the cross-file fault. Qualified calls and matching AST shapes were necessary.
Both legal and illegal programs are tested in both source orders.

A proposed overload control also corrected the hypothesis: nsc rejects
`def overloaded: Array[Int]` beside `def overloaded(i: Int): Int` when used as
`overloaded(0)`, rather than necessarily choosing the ordinary method. General
applicability and ambiguity of such result-valued overloads remain a separate
boundary; this batch does not claim to resolve it. The original oracle probe
and diagnostic are retained in the local batch evidence.

## Tests and evidence

`fieldscope` has six test groups and 34 acceptance/rejection conditions. Each
condition runs the real scalac 2.13.16 and scala-rs; every accepted program runs
under `java -Xverify:all` and compares stdout byte for byte with both the oracle
and a checked-in expected file. Separate compilation covers all four producer /
consumer combinations, plus rejected generic assignments.

Controls include primitive and bounded fields, Unit, arrays, reference and value
classes, inherited/imported/unqualified stores, local variables, receiver and RHS
evaluation counts, lazy initialization and exception retry, abstract inherited
properties, explicit empty method clauses, and illegal index types.

The first 25-condition matrix matched nsc after the four initial fixes; the
immutable pre-batch candidate disagreed in 14 conditions. Separate-compilation
positive clients failed with that same pre-batch binary and execute correctly
after the additional getter repairs. The accepted-before storage and source-order
witnesses are retained with the preceding batch inventory.

Before broad validation, pinned sources, 121 jar archives, 33 Java support classes
and 1498 reference class hashes were checked. The first source compilation
recovered Slick to zero errors and 1504 classes; library errors were 419 versus
the accepted 421, with no added diagnostic locations or messages. These are
prerequisite observations, not a merge verdict. Full gate results are recorded in
`tests/BASELINE.md` with the exact compiler commit and corpus ledger.

The related prerequisites retain verify_sql, lexicalcontext, macro/owner and
constructor/override suites, historical corpus regressions and the full negative
corpus. The first related run exposed empty-call regressions in btargs and
collectionresults: Java/loaded method signatures may lack an explicit
empty-clause marker. Receiver insertion now retains the method application
when there are no arguments. The failed run was stopped through its owned
process IDs, and these suites were moved ahead of the broad prerequisite run.
No full merge gate was spent on that intermediate failure. A bidirectional
Java and prelude probes then exposed false acceptance of
`"abc".toCharArray(0)` and `Iterator(Array(7)).next(0)`. Descriptor/prelude
methods may lack reliable clause markers. Getter insertion therefore uses
a record of actual source clauses or loaded Scala property identity
(including a mutable getter and its matching storage field); neither a zero
parameter count nor the absence of Java flags alone proves a Scala
parameterless declaration. The
explicit Java empty-clause call and both illegal empty-clause shapes are
checked against scalac.

A failed prerequisite stops before the full merge gate. Existing tests
are not removed or weakened.

## Remaining boundaries

Value classes with Object-erased underlying storage and Java Object[] origin
through exported Scala signatures need separate ABI probes. General overloaded
getter-result application needs applicability and ambiguity analysis. Loaded
non-accessor methods whose clause markers have been lost also need declaration
metadata recovery; this batch does not guess their source clause shape. Passing
these field tests does not establish full Scala compatibility, macro owner
conformance, or successful Gitbucket/cats compilation.

The broader receiver rewrite initially bypassed open type-parameter tracking
and failed the existing partfactory source/binary test. The final implementation
extends `insert_apply_on_nullary`, which already preserves that tracking, to
Array results instead of duplicating the Class-result path. Factory suites are
run before the broad prerequisite list. The failed related run stopped after
100 completed suites (1239 passed, one failed); it was not a full gate.

The final related command uses only existing targets. An unavailable `lowerinst`
target in a scratch prerequisite command stopped before running any factory
tests; it was removed from that command, with the compiler binary unchanged.
Previously passed phases were retained by hash and the command resumed at the
failed phase. The final partfactory/memberbatch run passed before broad tests.

Final prerequisites completed all 12 phases: 1615 CLI tests across 153 suites,
198 typer tests, 1986 selected pos/run corpus identities (zero losses, eight
gains), and all 1405 negative identities (zero losses, five gains). Format and
clippy comparison pass with the same 57 existing warning occurrences. The final
source check reports Slick 0 errors / 1504 classes and library 415 errors versus
accepted 421, with six removed diagnostics and no added diagnostics. Strong JVM
verification loads all 1504 Slick classes, with zero failures/incomplete classes;
all 12 runtime clients match nsc across 36/36 attempts. These precede the full
gate; they do not stand in for its result.
