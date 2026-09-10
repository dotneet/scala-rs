# Macro transport integration inventory

This batch starts from rejected candidate `2970a6a5`, after merging the latest
main ledger record `95ccb3bc`. The accepted compiler remains `23031519`.
The earlier type-identity/implicit batch is retained as one composed candidate;
no part of this batch has passed the merge gate yet.

Diagnoses below are hypotheses. Correcting them with measurements is the most
valuable result of the investigation. No compiler-wide before run is needed.

| Candidate | Evidence | Mechanism and dependencies | Difficulty / selection |
|---|---|---|---|
| Block arguments and local methods | `pos/annotated-original` regressed after source macro export | Missing outbound AST nodes and `c.untypecheck`; also exposes invalid bytecode in an implementation compiled by scala-rs | Medium, selected |
| Attachments through typecheck | Both `attachments-typed-*-ident` tests regress | Ident becomes Select after reverse RPC; the JVM original's attachments must survive the adaptation | Low, selected |
| Silent overloaded method query | `pos/t7461` regresses | **Corrected hypothesis:** scalac returns EmptyTree, not an overloaded type. A remaining overload is a failed TERM query | Low, selected |
| Repeated macro arguments | `pos/t8013` regresses with reflection argument mismatch | Parameter slots differ from actual argument count; the repeated slot must be a Scala Seq of Expr/Tree values | Medium, selected |
| Source positions | Source-reading interpolator in `t8013` | NoPosition cannot answer source/line/column; carry source text and real offsets | Medium, selected |
| Local class roundtrip | `pos/t9392` regresses | ClassDef/Template/constructor transport, untypecheck and local type identity | High interaction with AST transport, required to recover the batch |
| Private binary macro access | Both accepted230 and rejected2970 execute an external private macro call; scalac rejects | Macro-only supply precedes ordinary visibility filtering; retain declaration access flags/boundary | Low, selected; both API producers required |
| Whitebox implicit macro | Both accepted230 and rejected2970 print fallback99; scalac prints77 | Binary supply drops whitebox candidate; full expansion can refine inference results | High, deferred separately; no fake blackbox binding |
| Cats tuple swap | Two real cats diagnostics compare Tuple2[C,B] against tuple(C,B); standalone direct/function/dimap probes all match scalac | Full-run symbol provenance still needed; a blanket prelude edit is unsupported | Unresolved, deferred |
| Gitbucket implicit result | Prior composed candidate removes GetResult[Int] at IssuesService.scala494 with no added gitbucket diagnostics | Parent completion + implicit candidate selection, retained in composed batch | Already implemented; retain related evidence tests |

Validation first groups both API producers (scalac and scala-rs) with both
consumers, executes valid outputs with `java -Xverify:all`, and compares exact
stdout. Independent negative programs exercise visibility and argument types.
The frozen accepted binary must fail at least the repaired runtime/acceptance
behaviors. Corpus selection must inspect source contents and numbered compilation
rounds, not rely solely on names containing `macro`.

The six prior losses and the existing engine, macro, reification, implicit,
symbol-supply and backend regression suites are prerequisites to one full
composed gate. Record every rejected gate through DONE; only PASS with zero
corpus losses can move the compiler on main.

Additional measured seams found by the grouped probes:

- Private macros from scala-rs-produced APIs also bypassed the macro path via
  the flat eager subset reader. `javap` confirms that the API has no such JVM
  method. Exclude macro declarations from that reader and let the full pickle
  supplier install their binding/access metadata.
- `annotated-original` reaches a VerifyError in scala-rs's macro implementation:
  `Aliases.Expr(Tree, WeakTypeTag)` was emitted with only the tree argument.
  Materialization was incorrectly dependent on a wildcard universe import even
  for the explicitly qualified `c.Expr(...)` call. Use the qualified receiver's
  universe while filling that operation's implicit arguments.
- The first four recovered corpus cases are attachments (two), silent overload
  typing, and repeated interpolator arguments. These focused counts are not a
  replacement for the complete merge gate or execution comparisons.

Focused results before the broad prerequisites:

- The complete API producer/consumer matrix passes, including private rejection,
  empty/multiple repeated arguments, positions, attachments, block/function
  evaluation and local classes. The inferred Context.Expr implementation also
  passes both producer/consumer directions and its independent negative probe.
- The two engine tests which expected old block/function/new refusals retain
  their remaining singleton-tag/receiverless-prefix assertions. Their newly
  supported cases are copied into an executable fixture: stdout is `2,2,2,x,1`
  under scalac and scala-rs, including exactly one constructor side effect.
  The historical claim that carrying a written new necessarily evaluates it
  twice was not reproduced.
- **Separate existing limitation:** nsc cannot read the explicit dependent
  parameter/result signatures of scala-rs-produced `EgImpl`/`ExImpl` methods.
  Both the accepted230 binary and this candidate reproduce the same incompatible
  macro-implementation-shape error. The legacy transport test uses nsc/nsc,
  nsc/scala-rs and scala-rs/scala-rs; it does not claim the fourth combination.
  The dedicated source API matrix above does test all four combinations.
- Rebuilt accepted230 binary SHA256
  `99931c2e59d94fc7c5ba79b5e04633469cdabc7ff52da255555ecf9565d7e48c`:
  rejects the valid block transport fixture, accepts the invalid private macro
  call, and emits an inferred implementation that fails `java -Xverify:all`
  with an operand-stack-underflow VerifyError. Evidence is under
  `/tmp/scala-rs-macro-transport/before/`; this was focused before probing, not
  another aggregate baseline measurement.

### Prerequisite follow-up: storage and argument-clause semantics

The source-selected prerequisite (452 pos/run identities) recovered all six
2970a6a5 losses but exposed t12576 and macro-expand-nullary-nongeneric. The full
1405-case negative prerequisite exposed macro-blackbox-dynamic-materialization.
These were found before another full gate, not in a rejected merge gate.

The composed follow-up repairs:

- Missing trailing empty argument clauses are normalized only after typing has
  auto-applied them. Missing nonempty clauses are still rejected. Explicit empty
  applications, nullary applications and curried empty clauses execute against
  both API producers and both consumers.
- Package objects are adopted before wildcard bindings are populated. Macro
  declarations have no JVM method and therefore require the full pickle supplier.
  Standalone wildcard/qualified package-object macros execute; private package
  macros remain inaccessible. The old broad module-adoption regression t5639
  stays accepted.
- Reverse typecheck replies retain structural FunctionN, TupleN and Array type
  arguments. The fixture compares their reflection types with real scalac before
  producing its runtime result.
- ScalaSignature contains concrete non-lazy storage fields, including ordinary
  constructor and body vals/vars. Their private trailing-space names and direct
  types match nsc. Accessors retain NullaryMethodType and constructor accessors
  retain PARAMACCESSOR; their setters carry that flag as well.
- Storage does not inherit accessor-only flags. A prior follow-up copied IMPLICIT
  to the backing field and the existing typeidentitybatch test reported duplicate
  doubleEvidence candidates. Real nsc reflection confirms only the getter is
  implicit. Storage now retains field modifiers only. Getter PARAM/DEFAULTPARAM
  and mutable constructor-argument flags are also removed: constructor storage
  can be mutable while the constructor's parameter remains immutable.

`macrotransportbatch` now has five tests, including the full producer/consumer
matrix for storage-driven macro rejection and reflection flags. Both valid and
invalid programs are checked; valid outputs run with `java -Xverify:all` and
match scalac stdout. Accepted230 still omits all four ordinary storage fields in
an independently compiled reflection probe (`before-fields/`), and the nsc vs
scala-rs constructor-parameter probe found the pre-existing `var x` metadata.

Evidence root: `/tmp/scala-rs-macro-transport/`.

- `prerequisites-v2/` freezes binary SHA256
  `997420c09790a6cdfad98f2f431ca7fe6cd1d94bb7b3e7317cb2c966d88ac69d`,
  before the final flag corrections. Pos/run: 452 rows, one loss (t12576),
  20 changes. Neg: 1405 rows, zero losses, 11 changes. These are selected
  prerequisites, not whole-corpus counts or validation of the later corrections.
- The grouped existing suites found the duplicate implicit backing field and
  stopped; no full gate was started. After the flag corrections,
  `three-fixtures-v3.log` passes 57 tests in eight suites, including the failed
  suite, the remaining suites, all five new matrix tests and affected pickle
  suites. This does not claim a fresh whole-workspace pass.
- `clippy-v3-compare.json` has before=57, after=57, added=[], removed=[].
- `prerequisites-v2/preflight.log` validates four pinned source trees, 121 jar
  archives, 33 Java support classes and 1498 reference classes.

### Remaining integration loss: t12576

Name resolution is repaired. The macro now really expands and reaches a deeper
unsupported operation, Context.internal. It typechecks a function around the
already-typed argument, then changes local definitions' owners from
c.internal.enclosingOwner to that function's symbol. The old flat macro reader
accepted the call without expanding it; preserving that phantom method is not a
valid recovery.

`owner-inventory/` establishes nsc's actual contract: enclosingOwner is the
initializing val (Main.field or Main.local), the anonymous function has a real
method symbol owned by that val, its parameter is owned by the function and has
Int info, and changeOwner really moves the argument's local definition. The
anonymous function's own info is NoType. The valid program prints 42 twice.
`owner-info/` is an independent negative probe: forcing the inferred enclosing
val's info during its own macro expansion reports recursive value needs type.
Returning an uninitialized dummy owner would silently accept this invalid case.

The next correction needs faithful symbol identities, lexical ownership for
macro arguments/typecheck replies, and lazy owner-info completion or an explicit
failure when such information cannot be supplied. A no-op changeOwner or a dummy
NoSymbol/NoType owner is not acceptable. Keep this difficult protocol change
separate during implementation, then validate the composed batch (all existing
regressions, the two ownership probes and the complete gate). Main remains at
accepted95ccb3bc; this work is not merge-ready and has no new full-gate verdict.
