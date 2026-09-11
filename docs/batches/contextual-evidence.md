# Contextual evidence and declaration identity batch inventory

Base: local main 6557829d, accepted compiler e608c7dc; immutable binary
SHA-256 895222eada7b741eb3a7eeb5bdc58ef349bba9da63a13cb60e4a886f8e7a51a1.
The accepted baseline is gitbucket 96/45, cats 83/34, library 415/113,
Slick 0/1504, workspace 2783/0, corpus pos1150/neg718/run697.
No aggregate before remeasurement. No subagents. This inventory precedes
compiler edits. Every diagnosis is a hypothesis; correcting it through
measurement is the most valuable outcome.

| Candidate | Real scalac / accepted-before evidence | Hypothesized mechanism | Difficulty |
|---|---|---|---|
| Explicit parent constructor implicitly argument | nsc executes 7; rs refuses method (T)T; incompatible evidence rejected by both | Parent argument typed without selected formal prototype | medium |
| Inferred generic parent through subtype arguments | nsc executes 5; rs refuses Applicative[F]/Monoid[A] | Constructor selection precedes class parameter inference | medium |
| FunctionN class through higher-kinded identity | nsc executes Function1 and Function2; rs refuses generic consumer; direct structural function control passes | Structural/Class function correspondence missing in inference while conformance already recognizes it | medium |
| Local import in local method signature | nsc accepts wildcard and named import and runs; rs cannot find Event; later/import-outside-scope negatives rejected by both | All local signatures pretyped before source-order imports | medium |
| Generic invariant getOrElse fallback | nsc accepts and runs Set[A] and exact JDBCUtil reduction; rs widens Set[_ <: A]; wider/default-invalid controls agree | Prototype does not use receiver-substituted lower bound of fallback type variable | medium/high |
| Value-class this into direct argument and local slot | Both compile; nsc 7/text; rs ClassCastException; generic Box[Cell[T]] control agrees | Box retained across a declared unboxed value-class storage boundary | medium |
| Java generic Object override | nsc accepts Map[String,AnyRef] override and runs; rs says class must be abstract; wrong key type rejected by both | Java generic Object argument read as Scala Any instead of AnyRef | medium/high; past HeapBackend regression must be covered |
| Method F collides with inherited type parameter F | No-inheritance control agrees; inherited generic Base causes rs Tuple2[F,G] instead of App[F]/App[G] | Member selection namespace and inherited type parameters | medium |
| Tuple.swap identity | Structural and HK Tuple2 clients agree; full cats diagnostic remains | Full-run origin/normalization interaction not reproduced | unresolved, no speculative repair |
| Function8.tupled | Both compile and execute 8 | Simple arity hypothesis disproven | no repair justified |
| IterableOnce.reduceOption | Both execute 6/true | Missing-member hypothesis disproven outside full kernel compile | unresolved, no speculative repair |
| Result-valued overloads | Previous measured nsc ambiguity vs rs ordinary method; no relevant intervening change | General applicability through result.apply | high interaction, separate |
| Source-class macro universe mapTo | Existing full gitbucket diagnostic persists for source symbols | Macro transport still supplies placeholder classes | high, needs real universe integration; no stub workaround |

Evidence: inventory-results.json (15 program pairs) and more-results.json
(8 further pairs), source and per-compiler stdout/stderr under probes/ and more/.
Positive programs are compiled by real scalac 2.13.16 and executed with
java -Xverify:all. Negative pairs compare rejection in both directions.
Keep all supported low/medium repairs in one implementation batch, then run
existing boundary/regression prerequisites before the single composed gate.

## Implemented batch and corrected hypotheses

- Contextual parent constructor arguments use an agreed formal prototype from
  explicit parent type arguments during the body pass. Signature-only typing
  does not commit implicit argument trees that would be retyped without their
  inferred type arguments on the next pass.
- Generic parent arguments are inferred through the actual argument's base
  types before choosing a constructor. Only one validated solution is accepted.
- Structural function inference normalizes class-shaped Function1/Function2
  arguments through the existing function class shape. No new implicit view is
  inserted and ambiguous constructors are not guessed.
- Local method signatures see preceding named/wildcard imports. Local aliases
  following imports are completed in the same source-order signature walk;
  imports are restored before executable statements are typed.
- A contextual prototype may mention the enclosing declaration's fixed type
  parameters. Callee variables still stay open. Receiver-substituted lower
  bounds supply fallback argument prototypes when no result expectation settles
  them; a wider actual argument retains the existing retry without the hint.
- Term selection filters the term namespace before dropping inherited members,
  preserving a method F beside an inherited type parameter F.
- Value-class this is unboxed at declared local and formal storage boundaries,
  including constructors and branch results. Declaration metadata distinguishes
  Object-backed value classes from generic Box[Cell[T]] and Any slots, which
  retain the box. The initial argument fix exposed a second error: the generic
  boxing hint classified a declared Object-backed value class as a generic slot.
  The same declaration identity now controls both decisions.

The first combined implementation agreed with scalac on 20/23 probes. The
remaining parent evidence, invariant JDBC fallback and value-class argument
cases corrected the original partial diagnoses; the final combined matrix
agrees on 30 cases. One additional path-dependent local-val type-alias import
still disagrees and is not claimed fixed.

The Java generic Object hypothesis was disproved by a reverse boundary probe:
scalac accepts BOTH Map[String,AnyRef] and Map[String,Any] overrides of the Java
Map<String,Object> method. Simply narrowing nested Object to AnyRef regressed
that accepted Any override, consistent with the prior HeapBackend warning.
That implementation was removed in full before prerequisites. A later change
must model Java's special Object compatibility rather than widen/narrow every
Scala generic argument. The wrong-key override remains rejected by both.

## Reproducible validation

Four ctxev E2E groups cover 25 positive programs and six negative fixtures.
They compile real scalac 2.13.16 and scala-rs, execute every positive program
with java -Xverify:all, and compare stdout bytes with both scalac and checked-in
expectations. Positive programs are grouped into four compilation units to
amortize oracle startup. The immutable accepted-before binary rejects the
contextual, import and fallback units; it compiles the value-class unit and
then throws ClassCastException. Individual before/oracle probes preserve the
witness for each mechanism rather than relying only on combined failure.

Before a full gate, run existing constructor, import, inference, value-class,
collection, by-name and shared boundary suites. The prerequisite corpus
selection includes all 44 prior full-gate loss names found in retained ledgers,
the most recent by-name/value-class losses, and source matches for these
mechanisms (487 selected names in total). Input preflight checks four pinned
source trees, 121 jar archives, 33 released Java classes byte for byte and
1498 known-successful scalac reference class hashes. No aggregate before was
remeasured. A full gate, its result and its exact tree are recorded separately
in tests/BASELINE.md only after it finishes.

Java Object compatibility, local-val path-dependent alias imports, full-run
cats Tuple.swap/IterableOnce diagnostics, result-valued overloads, source-class
macro universe transport, MODE=a and specialization remain unresolved. This
batch is not a claim of complete Scala conformance or zero benchmark errors.

The first prerequisite pass completed 223 tests in 38 CLI suites and 190 typer
unit tests. Among 674 selected corpus identities it found run/t9223b as a
pass-to-fail regression, before any full gate. The new parent prototype was
being applied to a Function0 thunk already synthesized by the signature pass,
as if it were the raw String value. Existing zero-argument thunk arguments now
retain the selected by-name formal's checking path. A counter-based parent
client covers strict and by-name subclass arguments and repeated forcing.
The failed prerequisite remains preserved at prerequisites/; final results
must refer to the rebuilt compiler and final-prerequisites/ instead.

Final prerequisites passed 706 CLI tests in 40 suites (including conform and
e2e), 190 typer unit tests, and 674 selected corpus identities with zero losses.
run/t9223b recovered; run/t7859 and run/valueclasses-pavlov improve relative to
the accepted baseline. Clippy remains at 57 existing warning occurrences with
no added identity. The final binary hash and complete per-phase results are
preserved in /tmp/scala-rs-contextual-evidence/final-prerequisites/.
