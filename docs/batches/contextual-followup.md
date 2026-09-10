# Contextual invocation follow-up batch

Main at start: `cbc8320f` (accepted compiler and ledger: `b969b0d1`).
The rejected `ef3551c6` tree is preserved in its original worktree and branch.
This follow-up starts at local main and merges that candidate in `1126c717`.
No aggregate before measurement was repeated and no subagents are used.

## Inventory and selection

All diagnoses are hypotheses. Correcting a hypothesis with measurements is a
primary outcome; matching diagnostic wording is not proof of a shared root.
The earlier inventory covers thirteen families across gitbucket, cats, the
standard library, and silent acceptance/runtime errors. This batch retains
that implementation and adds the supported low/medium follow-ups below.

| Family | Measured evidence before selection | Decision |
| --- | --- | --- |
| Twirl binary overloads | Actual twirl-api 2.0.9 jar: scalac and accepted before execute Seq[Any], Any-typed null and Int calls; ef3551c6 rejects them | Recover precise pickle signatures and contextual specificity |
| Source AnyVal / Any overloads | scalac executes; accepted before rejects | Same applicability/specificity boundary; include Java calls and AnyRef controls |
| Top-type false acceptance | Passing String to AnyVal and Int to Scala AnyRef is accepted before, rejected by scalac | Remove adaptation bypasses; keep actual subtyping and conversions |
| Inferred val initializers | Full-gate loss pos/val_infer; executable inherited def/val and generic cases expose missing expected typing | Type initializer against inherited expectation while preserving narrower inferred results |
| Named curried constructors | Actual gitbucket API pattern reduces to named first clause and positional second clause; before rejects | Exclude inherited constructors and retain clause boundaries |
| Constructor defaults and evaluation | Missing first/later defaults, earlier-parameter references, lexical shadowing and observable order | Use real getters, preserve per-clause order and share earlier values |
| Separately compiled constructors | Real scalac-built class rejects omitted named defaults despite complete pickle flags | Preserve source symbol clauses alongside the flattened JVM method type |
| Symbol reference identity | Existing runwrong regression exposes unresolved Symbol literal type after removing the AnyRef bypass | Resolve the actual scala.Symbol class; compare overload selection and reject AnyVal |
| Inherited declaration order | pos/t1001 regresses with a shared broad ancestor preceding a narrower parent declaration | Use Scala linearization for inherited expectations |
| Case companion tupled | Explicit Function2 inheritance works in both compilers; prior apply.tupled reduction is invalid in scalac | No repair claimed; retain actual repository/import context for later investigation |
| Generic parents, dependent inner classes, Either inference, Slick shapes, source reflection | Existing inventory shows interacting constraints or unreduced actual-source contexts | Keep separate until a bounded measured repair is established |

## Corrected diagnoses

The actual Twirl signatures are `(AnyVal)T` and
`(Any)(implicit ClassTag[T])T`, not two Any domains. Runtime reflection of the
real jar distinguishes them even though their first JVM arguments are Object.
A trace of the compiler's supplied alternatives showed the second candidate
still carrying the erased Any type. `erased_param_desc` treated AnyVal as an
unknown reference slot, preventing unambiguous pickle recovery beside String
and NodeSeq overloads. Any, AnyRef and AnyVal have definite Object slots.

Source overload tracing found another boundary: the initial applicability
pass avoided views, but the specificity comparison enabled them again. Keep
that choice across the two stages, while retaining numeric weak conformance
for signature comparisons. Distinct residual clauses remain distinct; the
fix does not hide genuine overload alternatives again.

Abstract vals use `deferred_val`, not `method_is_deferred`. Both forms must
supply the inherited expectation. Ordinary inference retains a narrower
initializer result; the optional infer-override source feature keeps its
separate behavior. Written incompatible annotations are still rejected.

Flattening constructor lists before assigning names erased their boundaries:
a name from the second clause could incorrectly satisfy the first. Constructor
parameter names are not inherited. Placement now keeps each clause's names,
positions and defaults separate before JVM flattening. Runtime comparison
also caught a prior default being evaluated twice when another getter reused
it; cloned getter arguments must read the same saved value.

The binary constructor hypothesis was corrected: the observed constructor is
already unique and descriptor-linked, and its DEFAULTPARAM flags are present.
`supply_ctor` itself replaced source clauses with one flattened symbol list.
The symbols now retain source clauses while the method type remains flattened
for JVM constructor selection. Both source and independently compiled defaults
are executed against scalac, including omitted first and later clauses.

The first prerequisite pass found two regressions before a full gate: the
Symbol literal's unresolved Named type was masked by unconditional AnyRef
adaptation, and a DFS inherited-result walk reached a broad shared ancestor
before a narrower declaration. These now use the actual reference class and
Scala linearization respectively. Sentinel node identities are excluded from
cloned-default sharing.

## Prerequisite evidence

`crates/cli/tests/contextualfollowup.rs` contains six groups with 32 oracle
programs plus warm-loading/library units. All valid programs execute with
`java -Xverify:all`; stdout is compared byte for byte against real scalac
2.13.16 and checked-in expectations. Both acceptance directions are checked.
Fifteen of the 24 initial standalone programs and four of the six added
boundaries expose a discrepancy in the accepted immutable before executable. The real Twirl regression instead
fails on the rejected predecessor. The previous 33-program suite also passes:
65 programs total, thirteen test groups. The existing four runwrong tests pass
again. These are focused checks, not a merge verdict.

Source/jar/cache preflight passes: four pinned source trees, 121 jar archives,
33 Java classes matching released jars and 1498 known reference classes.
The first combined prerequisite pass completed: 1599 related CLI tests passed
and one failed (runwrong); typer tests passed 198; 1985 selected pos/run corpus
identities had one loss (t1001); all 1405 negatives had zero losses and five
gains. That rejected intermediate executable is SHA-256
`4c5565ad71758fd7` (prefix; full hash in prerequisites/binary.json).

After the combined repairs, all 30 affected/mandatory CLI suites passed
(707 tests), as did 198 typer tests. The 1985 selected pos/run identities
have zero losses and five gains; all 1405 negatives have zero losses and five
gains. JSON clippy comparison retains the same 57 warning occurrences,
with no added or removed identities. The combined waiter reached DONE with
all phases exiting zero. The frozen executable SHA-256 is
`b340dac72378160837166bde7d7aeb86c0e5f858b039846f5ce21daf27d4f4b7`.
Evidence: `/tmp/scala-rs-contextual-followup/`, including `prerequisites/`,
`probes/` and `binary-constructors/`; Twirl evidence is also retained under
`/tmp/scala-rs-contextual-inference/twirl-regression/`.

This is a candidate handoff, not an accepted merge. The final composed tree
must pass the owned full merge gate and an independent diagnostic audit,
with zero corpus losses, before main can receive the compiler changes.
The gate result and raw corpus ledger are recorded in tests/BASELINE.md.

A source-owned scala.Symbol boundary without inheritance executes identically
in both compilers. Adding a subclass reveals classpath metadata contamination,
but the immutable accepted before reports the same four diagnostics. This is
a separate pre-existing source/binary ownership issue, not a repaired outcome
of this batch. Evidence: source-symbol-child-results.json and
source-symbol-child-before.log in the scratch evidence directory.
