# Recovery batch inventory (written before its implementation)

Preserve frozen rejected candidate 18b8e5d7 and finish its gate through DONE.
No source edits while its gate runs; no subagents. Base measurements remain
accepted e608c7dc (main 6557829d); pending measurements are NOT a baseline.
Diagnoses below are hypotheses to correct through measurement.

| Candidate | Real scalac / accepted-before / candidate evidence | Mechanism hypothesis | Difficulty |
|---|---|---|---|
| Either.fold with identity | scalac and before execute 7/8; candidate refuses Single[A] vs Positioned[A]; explicit x => x passes all | Class FunctionN unification fixes a result variable from identity before seeing the other lambda; preserve common expected result and both constraints | medium/high, mandatory regression |
| Nested higher-kinded expected result | scalac and before execute 7; candidate rejects Now[(E,S)=>Eval[(S,A)]] | Newly structural FunctionN inference pins F to Now despite expected State[Eval,...] | medium/high, mandatory regression |
| Lower-bound prototype triggers a value conversion | existing mapkey runtime expects updated value 2/default 3; candidate prints a/a by using implicit intKey on VALUE positions | Receiver lower bound is only an inference hint; applying implicit views to satisfy it prevents legitimate value widening. Key positions must still allow conversions | medium/high, silent miscompile, mandatory regression |
| Generic factory implicit evidence | scalac executes 7; before/candidate both refuse Ev[A] | adapt_implicit_apply_in filters TypeParam solutions even when they denote fixed enclosing variables rather than callee inference variables | medium |
| Same-arity secondary constructor | scalac executes 7; before/candidate reject Chain[A] required NonEmpty[A] | new-path adaptation replaces selected secondary parameters with primary ctor_fields whenever class arguments are inferred | medium |
| Local val alias import | scalac executes 7; before/candidate refuse Event alias as E | Prefix/type-alias identity or early signature completion, trace before editing | medium/unresolved |
| Java generic Object | nsc accepts both Any and AnyRef overrides; before rejects AnyRef; narrowed experiment rejects Any and was removed | Java-specific Object type equivalence, not blanket AnyRef narrowing | high; preserve HeapBackend and Scala invariant controls |
| Tuple.swap / IterableOnce / NonEmptyList cons | full-run cats diagnostics persist; simple Tuple/IterableOnce probes agree | Context/origin-dependent, still unproven | unresolved |
| Source-class macro universe / result-valued overloads | existing diagnostics/ambiguity probes persist | real source type universe transport / general overload applicability | high, separate |

Evidence: fold-regression/results.json; next-probes/results.json; boundaries-results.json.
The first two new failures are both reduced, avoiding whole-project bisection.
Keep original expected type, implicit clauses, declaration identity and source
scope visible in traces. Add bidirectional negative controls before accepting
changes. Run the existing 674-identity prerequisite selection, all relevant
CLI boundaries, and the exact reduced fold/nested-context clients BEFORE a
full gate. An early Slick compile is warranted for this recovery because the
last general inference batch passed narrow suites but regressed StatementInvoker.
Do not pay another full gate for its one repair alone: include the confirmed
factory and constructor repairs in the same batch when their probes validate.

Gate workspace caught mapkey::map_key_acceptance_and_execution_match_scalac.
The initial selected CLI list omitted mapkey even though getOrElse was changed.
Run mapkey FIRST in recovery prerequisites, plus collectionresult/memberbatch,
mapredirect and all suites whose fixtures exercise getOrElse/updated. The
implicit intKey conversion is valid for keys but must not convert widened Map
values. The plain wider-default probe had no conversion in scope and missed
this. Record this omission rather than describing the initial prerequisite
coverage as comprehensive.

Additional controls in next-boundaries/results.json: explicit type arguments
repair both inference regressions; explicit factory arguments already agree;
all five wrong-type/missing-evidence cases are rejected by nsc/before/candidate.
A side-effect counter confirms nsc's primary/secondary constructor selection;
both current binaries reject it. No recovery implementation has been made.

Local compiler source reference for follow-up analysis:
/tmp/scala-rs-corpus-20260910-codex/src/compiler/scala/tools/nsc/typechecker/
Typers.scala:3563 (typedArg), :3980 (handlePolymorphicCall lenient/strict
prototypes and inferArgumentInstance); Infer.scala:449 (protoTypeArgs,
variance-sensitive bounds rather than unconditional lower-bound substitution).
The observed runtime mismatch is authoritative; do not call a blanket
no-implicit-conversions flag a proven fix before bidirectional tests.
For constructor identity, check_namer.rs:1659 already recognizes a primary
constructor by params == class.ctor_fields. The new-expression path currently
uses ctor_fields unconditionally for inferred same-arity calls at
check_apply.rs:500 and :593, even when it picked a secondary constructor.

Final rejected gate: /tmp/scala-rs-gate-18b8e5d7-codex. See tests/BASELINE.md for all completed results.
