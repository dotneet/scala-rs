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

## Implemented recovery and corrected diagnoses

The recovery composes the seven mechanisms of rejected 18b8e5d7 with these
repairs; its final gate is still pending. The original inventory above is
retained as the pre-implementation record.

- A result variable constrained covariantly by a typed function argument is
  left provisional while a sibling lambda can still widen it. Function input
  or invariant constraints stay fixed: the broader probe `choose(f, () => 3)`
  with `f: String => String` caught an over-permissive first implementation.
  Explicit type arguments remain binding. The recorded fold regression,
  reversed arms, no expected result and function-valued identity controls
  execute with the real-scalac outputs.
- A single applicable-method candidate considers invariant result constraints
  before rejecting its arguments. The nested `State.applyF` failure happened
  during applicability, before the main inference pass could replace `Now`
  with the expected `Eval`. Multi-alternative overload ranking is unchanged.
- Receiver lower bounds supply provisional argument expectations. Their value
  positions cannot demand implicit conversions merely to fit the hint; written
  ascriptions and concrete nested formals still can. Hint provenance follows
  block/branch results and inferred nested formals, covering `id(9)` as well as
  literals. Map keys continue to use legitimate conversions. The first repair
  fixed literals but still converted the nested identity call; broader runtime
  comparisons corrected that partial diagnosis.
- Implicit factory result inference accepts fixed enclosing type parameters.
  It excludes unresolved variables owned by the factory rather than excluding
  every `TypeParam`. Wrong and missing evidence remain errors.
- Inferred same-arity secondary constructor calls use the chosen declaration's
  formal types. Only a primary constructor uses `ctor_fields`, retaining the
  prelude Tuple2 primary signature fallback. A counter checks which constructor
  executed and how many times.
- A written case-class `copy` suppresses the synthetic declaration. A concrete
  parent's `copy`, including a direct parent's private method, also suppresses
  it; an abstract declaration can still be implemented by synthesis. Both copy
  rewrite paths require an actual synthetic declaration. The curried rewrite
  restores an ordinary application when its speculative case-copy rewrite does
  not apply. The secondary-constructor repair exposed a phantom copy overload
  that code generation did not emit; early tests then exposed a constructor
  rewrite that bypassed the newly suppressed declaration.

The expanded ctxev suite adds four grouped positive units (23 independent
programs) and fourteen negative fixtures. The original four groups remain.
Every positive executes with `java -Xverify:all`, and stdout is compared byte
for byte with real scalac 2.13.16 and checked-in expected output. Dedicated
negative controls cover contravariant domains, invariant results, explicit
arguments, evidence, constructor overloads and suppressed copy declarations.

Evidence and binary/source snapshots live under
`/tmp/scala-rs-contextual-recovery/`: first/, second/, inference-controls/,
copy-controls/, value-prototypes/ and the owned prerequisite directories.
The first recovery prerequisite pass ran mapkey successfully, then stopped at
ctxev's private-parent-copy negative before spending a full gate. The failure
showed that removing the symbol was insufficient: the unqualified/qualified
copy-to-constructor rewrite also needed to respect synthesis suppression.

## Additional inventory findings kept separate

A function-valued by-name fallback,
`Map("x" -> (() => "ok")).getOrElse("missing", () => 3)()`, compiles but throws
an Integer-to-Function0 ClassCastException in both the accepted-before compiler
and this candidate; real scalac prints 3. Current typing conflates a source
Function0 value with a previously generated by-name thunk. This needs explicit
thunk provenance throughout adaptation/retyping and is not claimed repaired
by the provisional-value conversion change. It is a higher-interaction follow-up,
with its source, oracle, before and candidate output retained in value-prototypes/.
The Java Object, source-universe macro, local-val alias and full-run cats roots
from the broader inventories also remain separate and unresolved.

The final grouped fixtures also ran against both immutable old binaries. The
accepted-before compiler rejects the inference, factory and copy units; the
rejected compiler does likewise. The value-hint unit runs correctly before but
prints converted values under 18b8e5d7. Both old binaries wrongly accept all
six suppressed-copy negative controls. Current ctxev checks require their
rejection, alongside the eight other negative controls that already reject.
The rebuilt batch passed mapkey (1 test) and ctxev (8 tests), including the
ordinary and user-written curried copy clients. Clippy compares equal at all
57 warning identities and occurrences. Broader prerequisite results and the
full composed gate remain to be recorded after they finish.

Independent accepted-before probes of the two curried-copy clients compile
and execute successfully but print `()` where scalac and the repaired batch
print `7` (curried-before/results.json). This was a silent wrong answer in an
ordinary object as well as a case class: the failed rewrite replaced the call
with an empty tree. The transactional rewrite fixes that separate runtime
path, not merely an acceptance diagnostic.

## Completed prerequisites for the composed candidate

Validated prerequisites passed 714 CLI tests across 43 suites and 190 typer
unit tests. The corpus selection covers 2,304 unique identities, including all
1,405 negatives and all 44 historical loss names, with zero pass-to-fail losses.
The selection initially missed the plus-named reify test because a regex escape
was inappropriate for scala_corpus.sh's zsh glob. An identity audit caught it;
the missing test was run once as a supplement and the complete union was
compared against the accepted ledger. Both original and supplemented ledgers
are retained, rather than calling the first 2,303 rows complete.

Early Slick compiled all 184 inputs with zero errors and emitted 1,504 classes;
strong JVM verification loaded all 1,504 with zero failures or incomplete loads.
Clippy has 57 existing warning occurrences and no added identity. Environment
preflight passed four pinned source trees, 121 jar CRCs, 33 released Java
classes and 1,498 reference class hashes. These are prerequisites, not a full
gate or a claim that the candidate has been accepted onto main.
