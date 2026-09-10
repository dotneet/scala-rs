# Contextual inference and invocation batch

Base: local main `56bf357f` (accepted compiler gate `b969b0d1`).
The immutable before executable is SHA-256
`cf35c34a0c24c0c27481720074c7a41ada3c923f135cdce6193867826a39234f`.
No aggregate before measurement was repeated.

## Inventory before selection

The full remaining diagnostic inventory covers cats (90), gitbucket (105),
and the standard library (421). Every proposed root was treated as a
hypothesis; correction by measurement is an intended outcome. Twelve initial
families were compared against real scalac 2.13.16, followed by independent
boundary probes. Valid programs were executed with `java -Xverify:all` and
stdout compared byte for byte. Scratch evidence lives in
`/tmp/scala-rs-contextual-inference/{initial,extra,fixture-before}`.

| Candidate | Initial evidence | Mechanism / decision |
| --- | --- | --- |
| Eval inferred overrides | scalac executes; before reports ambiguity | Untyped implementing val does not yet hide inherited implicit; selected |
| Narrowed override result | Both execute | Initial covariant-result hypothesis did not reproduce; retained as control |
| Implicit residual overload | scalac executes; before chooses unapplied explicit clause | Contextual applicability and duplicate signatures; selected, including reverse declaration order and false acceptance |
| Implicit method eta expansion | scalac executes; before rejects | Implicit clause counted as function inputs; selected with generic, missing evidence, input and bound controls |
| Parent repeated arguments | scalac executes; before rejects | Element adaptation and real class/object super-call packing; selected |
| Stream.Empty | scalac executes; before rejects | Parents were present; standard-library class allocation discarded pickle variance; selected |
| Either branch result | scalac executes; before rejects | Contextual polymorphic inference; coupled, keep separate unless a bounded repair emerges |
| Invariant branch container | Both execute | Does not reproduce the complete OptionT context; no claimed repair |
| Same-named type/term member | Both execute | Complete Func context still needed; no namespace patch from this probe |
| Curried tuple + implicit | Both execute | Real Slick signature context still needed |
| Case companion apply.tupled | Both reject | Initial reduction was invalid; actual Function2 inheritance must be preserved |
| Upper existential argument | scalac rejects; before accepts | Capture constraint issue; separate difficult root |

Other inventoried families remain generic parent inference, dependent inner
classes, source macro reflection, real Slick OptionLift/Shape, Java Map member
identity, and full-load-only IterableOnce reduction. These require their actual
binary/source contexts; none is claimed fixed by an unrelated reduction.

## Implementation and additional runtime findings

- An inferred ordinary val hides an inherited parameterless implicit while its
  initializer is typed. The later override checks still validate its result.
- Method values with a final implicit clause expose only the explicit inputs.
  Their generated application performs normal inference and real evidence
  search. Receiver evaluation must happen once when the function is created.
- Overload deduplication must retain distinct residual parameter clauses.
  A value expectation can eliminate an explicit residual clause when an
  otherwise equivalent alternative supplies it implicitly. A following written
  clause cannot disambiguate the preceding overloaded application.
- Parent applications adapt each repeated element, including elements past the
  declared parameter count. Both class and object constructors use the existing
  JVM argument packing routine, preserving Scala sequences and Java arrays.
- A runtime boundary probe found erasure treating a same-named overload as an
  override and changing an Int return descriptor to String. Erasure now consults
  the pre-erasure override relation, rather than the name alone.

The Stream hypothesis was corrected twice: the hierarchy was already present,
and the missing variance came from the standard-library allocator using empty
flags, not from external JVM-placeholder completion. Only that allocator is
changed. A further negative probe showed inferred vals skipped override result
checking entirely; their known result types now undergo the normal check.

These are an implementation batch; focused and historical checks precede one
composed full gate. Validation results and handover provenance will be added
from the owned logs, never inferred from successful compilation alone.

## Prerequisite findings

The initial composed implementation passed 1,585 tests in 146 related CLI
suites and 198 typer tests. The 1,417-unit pos/run comparison caught three
losses before a full gate was launched: pos/t7776,
run/macro-basic-mamd-mi and
run/macro-invalidret-doesnt-conform-to-def-rettype. Correct binary covariance
exposed unresolved result-only parameters in materialized WeakTypeTag requests.
A real scalac control demonstrated that a covariant expected result does not
force the expected argument: the unconstrained parameter minimizes to Nothing.
The existing ClassTag-only continuation is shared with TypeTag/WeakTypeTag,
while preserving evidence-derived solutions, lower bounds and in-scope abstract
parameters. Permanent runtime and negative tag controls accompany this repair.

The three corpus regressions recovered (3/3 pass, zero losses) after extending
materialized-tag inference and retaining the outer call's solved bindings while
nested tag construction is typed. All 27 permanent Scala programs pass the real
scalac acceptance/rejection and JVM/output comparison in seven test groups.
This includes separately compiled Java parent constructors, zero/repeated/spread
arguments, receiver evaluation count, widened and narrowed Stream types, and
weak/strong tags with inferred, lower-bounded and explicitly scoped parameters.
The accepted before binary rejects the new tag minimization runtime program.
The final affected 28-suite run, 198 typer tests, selected corpus, full negatives,
format and clippy are recorded under final-prerequisites in the scratch evidence.
The full-gate result and exact handover commit will be recorded in BASELINE.md.

Final related checks: 612 tests in 28 affected CLI suites and 198 typer tests
passed. The repeated 1,417-unit pos/run comparison has zero losses and one
gain, pos/t927. The broad original 146-suite result remains valid for mechanisms
not changed by the tag follow-up. Both rounds are retained independently.

The full 1,405-unit negative prerequisite then caught neg/t6928: the old
compiler rejected the test only because it mishandled repeated elements, hiding
a missing module self-reference check. Real scalac probes reject direct, nested,
qualified and function-contained self references in super constructor arguments.
Despite the diagnostic's by-name wording, scalac 2.13.16 also rejects a by-name
self reference; Typers.analyzeSuperConstructor confirms the unconditional walk.
The parent checker now validates actual term references after argument typing,
excluding type-only positions. Valid by-name evaluation of a different object
and null/repeated controls are exercised at runtime. The rejected intermediate
negative ledger remains in final-prerequisites; the final parent follow-up
checks are kept separately in parent-prerequisites.

The final oracle suite contains 33 Scala programs in seven groups, all accepted
or rejected like real scalac, with successful JVM verification and exact output
for every valid case. Nineteen expose defects in the immutable before binary.
The three additional false acceptances are nested, function-contained and
by-name module self references; the ordinary different-object by-name control
matches scalac's deferred evaluation count. No full gate has been spent on an
intermediate failed prerequisite.

Final parent follow-up prerequisites passed: 527 tests in 22 CLI suites,
198 typer tests, 1,418 pos/run units (zero losses; pos/t927 and run/t6928-run
gained), and all 1,405 negatives (zero losses; neg/i10715b and neg/t473 gained).
Release clippy retains the same 57 warning occurrences with none added or
removed. Format and source/jar/cache preflight pass. The fixed binary is
SHA-256 `4cf69f47affaabbaa6d05af309683c7255339450d818a12ddc7bb83e375da4da`.
The clean committed composed tree is the next and only full merge gate for
this implementation batch. Its outcome is recorded in BASELINE.md.
