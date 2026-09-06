# Codex continuation: validation in progress

This is a progress record, not a completed compatibility claim. Recovery was
independently measured at `d9eb5dc` and merged into local main as `4b0568af`.
Read the updated `tests/BASELINE.md` rather than remeasuring the old tree.

## Current decision state

The sections below preserve earlier checkpoints; this section supersedes
their pending-status statements. All new subagents use Luna / xhigh and
isolated worktrees. They report through collaboration tools, not Codex task
messages.

Latest independent checks (these supersede the pending descriptions below):

- Bounds and alias declaration metadata are merged as `41375307`. Candidate
  `4b2f941e` passed 2234 workspace tests, every baseline compile measure,
  strict verification of all 1490 classes, and Slick MODE=b 36/36. MODE=a and
  the specialization ledger remain red. All 5324 corpus statuses are
  unchanged; only the existing `t8199` diagnostic's temporary path differs.
  Two permanent CLI interop tests were added and independently passed at
  `2b65cdc9`, with no compiler/Cargo changes after the full gate. These verify
  valid execution and every invalid existential/forward/F/lower type bound.
  The resulting main test inventory is 2236 tests / 198 result rows, validated
  by a full run plus that test-only supplement, not a claimed second full run.
  Clippy retains the preceding 59 warnings; the added tests introduce none.
  Evidence: `candidate-4b2f941` and `integration/bounds-parent` below the
  temporary evidence root. `tests/BASELINE.md` now records this merge.
- Variance metadata (`+A`/`-A`) independently passes its real-scalac/JVM
  interop test in metadata candidate `9e522013`, but is not yet on main.
  The newer specialization candidate `14476f75` combines bounds, variance,
  and local-symbol clone fixes. Its two bounds tests and six specialization
  tests pass; a seventh test expects scalac to specialize a lower-bounded
  call that scalac actually keeps generic once real bounds are pickled.
  Direct execution still produces the correct result, including the newly
  exercised `upper("hello")` generic CharSequence call. The agent is aligning
  specialization eligibility and the test with nsc's actual behavior before
  a new full gate. The successful old `560405fd` full run is not evidence for
  this newer candidate.
- Specialization candidate `560405fd` completed its full runner: 2234 release
  workspace tests pass; all four compile measures match the baseline;
  strict verification loads all 1490 classes; Slick MODE=b passes 36/36.
  MODE=a and the class-specialization ledger remain red and unchanged.
  All 5324 corpus statuses match `318c1568`; the only six-field difference
  is the temporary output path in the existing `run/t8199` diagnostic.
  Evidence: `candidate-560405f/results.json` and `corpus-parent-audit.json`.
  This candidate is still held: additional local-class probes require
  symbol-cloning fixes. Follow-up `e02b8161` is being reviewed for external
  constructor ownership and complete type-symbol remapping before acceptance.
  An independent bounded-type probe found a second new regression:
  `size[@specialized(Int) A <: CharSequence](a: A) = a.length` emits an
  invalid primitive method and fails JVM verification even for a String call.
  Main and scalac both print `5`; scalac warns that bounds prevent
  specialization. Variant generation and source annotation advertising must
  agree on eligible bounds. Evidence: `specialization-validation/bounded`.
  Clippy exits 0 but adds four warnings (59 to 63); these also remain pending.
- Combined metadata candidate `fbc4ca7e` passes 15 fresh focused release
  tests (codegen diagnostics, source signatures, constructor defaults).
  It is held because a separate producer probe rejects ordinary `case class
  Row(i: Int, s: String)` for missing `Equals.canEqual`, while accepted main
  compiles it. Do not classify that regression as a pre-existing defect.
  A different overload probe reveals an existing defect: a case class with
  `def canEqual(i: Int)` compiles on main but throws `ClassCastException` when
  called through `Equals`; scalac prints `true`. Both are assigned to the
  source-signature follow-up. Evidence is under
  `integration/source-signature-probes/{rows,case-overload}-results.json`.
- Gitbucket precision candidate `29e38dd1` passes four independently run
  release tests, including exact negative diagnostics and JVM runtime output.
  Its performance is not accepted. Candidate `554b4de1` combines this work,
  accepted main, and the proposed earlier divergence check `4c261de5` for a
  bounded investigation. Reordering that check alone is not proof of the
  claimed speedup or a fix for fresh inference variables in recursive rules.
  The independent fresh build passed, but the measurement timed out after
  60.008 seconds (exit -15) with no diagnostics. Evidence is under
  `gb-validation/investigation-554b4de`; no error count is inferred.
- Macro snapshot proposal `f4099fd7` is held: a constructor-field-only
  declaration list is not a complete case-class symbol graph. Generic/active
  symbols must not lose previously working name-only queries. Permanent
  reflection comparisons and a safe incomplete-snapshot path are pending.

Tail-call work was independently frozen and tested at `dd5047e0` in
`codex/tail-validation`, then merged locally as `318c1568`.
Session `41170` completed the full acceptance sequence; it does not include the
new backend diagnostic gate. Logs are under
`/tmp/scala-rs-codex/integration/candidate-dd5047e`. The Slick reference build
is reused from the recovery evidence directory, with separate candidate and
MODE-specific client directories. Do not start a duplicate full run.
The workspace phase passed 2233 tests with zero failures (197 result rows).
All four compile measures equal the accepted baseline, strict verification
loaded 1490/1490 classes, and MODE=b passed 36/36 attempts. MODE=a still has
12 compile failures and the specialization ledger is unchanged. All 5324
corpus identities are present, with no lost pass or new skip. Run passes rose
from 587 to 590: `t3761-overload-byname`, `t8893`, and `t8893b`. All three
were separately compiled and executed by the coordinator with both compilers;
their output bytes agree. The only other six-field ledger difference is a
temporary path in the unchanged `t8199` filename-too-long diagnostic. See
`corpus-parent-audit.json`, `corpus-detail-audit.json`, and
`runtime-parent-audit/results.json`. The current baseline records this merge.

`cargo fmt --all -- --check` passed on merged main. Clippy on the identical
recovery compiler (`cargo clippy --workspace --release`, session `97405`)
completed with exit 0 and 59 warnings; this is not a warning-free claim.
Its log is `candidate-d9eb5dc/clippy.log`.

- Recovery `d9eb5dc` has independently passed 2228 release workspace tests,
  all 1490 Slick class loads (zero failures or incomplete loads), structural
  lint, and Slick MODE=b execution: 12 programs, 36/36 attempts. Measurements
  are Slick 0 errors / 1490 classes, cats 350 errors / 81 files, gitbucket
  912 / 111, and Scala library 1613 / 171. Session `28152` completed with
  exit 0: the full corpus has 5324 unique six-field records, pos 1053/461/345,
  neg 659/377/369, run 587/920/553. Specialization remains 2 match / 26 differ /
  9 no-compile, with zero specialized classes. Recovery is merged; the new
  per-test baseline is `tests/baselines/corpus-4b0568af.tsv`.
- The original three separate branch corpus runs are complete and each has
  5324 unique, six-field records. Nullcross matches the recorded aggregate
  baseline. Cats and implicitmemo each lose only `run/t5923b` relative to the
  nullcross ledger: `new Array[Nothing]` has the wrong runtime class. The
  existing Null slice fix `83be6e6`, already in recovery, resolves that
  source-level mismatch in an independent JVM/scalac comparison.
- Slick MODE=a is newly measured: all 12 clients fail to compile against our
  classfiles. There is no recorded MODE=a baseline. This exposes the source
  signature limitation already documented in `docs/language-support.md`:
  parameter clauses have been flattened before pickling. A source metadata
  snapshot before lowering is being implemented in `codex/source-signature`.
  The same logs also expose existential/member metadata gaps; fixing clauses
  alone must not be reported as full MODE=a compatibility.
- New features remain separate in `codex/integration-next` at `1b067aab`.
  The tail-call fixes independently pass 28 focused release tests and the
  super-accessor correction passes 101. A fresh parent-built provider plus a
  scalac-built subclass raised `AbstractMethodError` for inherited
  `Layer$$super$foo` at `6f26485`. Metadata tests pass at `1b067aab`, but a
  fresh concrete-overload linearization probe still raises `VerifyError`.
  Generic dispatch identity must be preserved before erasure; a permissive
  abstract-parameter fallback is not accepted.
  An earlier ABI probe accidentally used the agent's stale release binary
  and is explicitly invalidated; only `parent-6f26485-*` evidence is current.
- Gitbucket import completion plus a conservative implicit candidate filter
  is at `3f4e170` in `codex/gb-validation`. Session `18153` completed the
  release workspace suite with 2231 passing tests and one diagnostic-text
  failure in `slickimpl`; gitbucket timing was not run. The changed diagnostic
  still rejects the invariant `BaseTypedType[Any]` assignment, but its type
  provenance must be reviewed before changing the expectation.
  A subsequent investigation-only measurement on the same compiler timed out
  after 180.006 seconds (exit -15), without diagnostic output. No result count
  is inferred. `codex/implicit-profile` investigates repeated search work
  independently of the member-type fix in `codex/gb-import`.
- The derived implicit member correction `b55cb7d2` is staged separately as
  `d34fd8de`. Parent release tests passed 30/30 across `gbimport`,
  `implicitmemo`, `innerclasses`, `outer`, and `slickimpl`. Permanent precise
  type regressions are being added before full validation. The three
  unreachable-pattern warnings in `check_infer.rs` also occur on accepted
  main; an initial attribution to this slice was incorrect. The timeout
  above remains unresolved.
- Method specialization is staged separately as `d80d235c`. Parent review
  found that cloning a specialized method incorrectly changed a recursive
  `f[String]` call into the primitive variant, causing `ClassCastException`.
  The follow-up passes four fresh release tests and the original standalone
  reproduction now prints `ok`, `ok`, `generic`, matching scalac. This is a
  focused result, not full workspace/corpus acceptance or class specialization.
  The additional ambiguous/unused type-argument correction and accepted tail
  changes are now frozen together at `560405fd` for the full acceptance runner
  (session `57064`, `candidate-560405f`). Do not mutate that candidate while it
  runs. Independent Array/List/Tuple/thunk/cast probes produce the same 15
  runtime results as scalac. A separate scalac client exposed the existing
  loss of `Array[A]` arguments in source pickles; it also fails on accepted
  tail main, so this is assigned to source-signature work, not attributed to
  specialization. Evidence: `specialization-validation/containers-results.json`
  and `container-consumer-baseline-results.json` under the common temporary
  evidence root.
- Current-run macro metadata now has an executable runtime-universe
  hydration probe and a completed-symbol snapshot design (`3839f888`).
  Incomplete/active symbols remain explicit refusals. Production snapshot
  serialization and hydration are being implemented; `mapTo` is not complete.
- External constructor default getters (`c74dd7b`, `1140fb6`) remain held.
  Generic and nested cases now have focused evidence, but assigning default
  flags to every overloaded constructor from getter names requires review.
  Follow-up `50995a4` replaces that heuristic with pickle parameter flags.
  Parent review requested correct owners for replacement parameter symbols
  and exact constructor association when a real overload resembles an extra
  hidden outer parameter. The follow-up is not yet independently accepted.

Machine-readable process/commit state is in
`/tmp/scala-rs-codex/integration/current-state.json`. Recovery logs are under
`/tmp/scala-rs-codex/integration/candidate-d9eb5dc`. The Slick runner now keeps
MODE=a and MODE=b client artifacts in separate `progs-a` / `progs-b`
directories: switching directions previously overwrote successful execution
evidence. Compiler outputs may still be explicitly reused. The coordinator
reran MODE=b with unchanged recovery output (36/36), then MODE=a for one
client: all 116 MODE=b artifacts remained byte-identical. Invalid MODE values
now fail with exit 2. See `slick-preservation.json` in the recovery evidence.

The two newly passing corpus identities have different explanations:
`t5629` is the generic-owner override-bound correction, while `t12478` is a
UTF-8 locale effect. The same recovery classfiles print the expected Unicode
under `C.UTF-8` or explicit `-Dfile.encoding=UTF-8`, and question marks under
`LC_ALL=C`. Do not attribute that second result to compiler implementation.

## Preserved implementations

| Work | Branch | Reviewed implementation checkpoint |
| --- | --- | --- |
| Implicit memoization | `agent/implicitmemo` | `64685d3` |
| Null erasure / foreign accessors | `worktree-agent-a44905be6f76c9f6a` | `8465dea` |
| Cats inference | `agent/catstail3` | `8f6bb72` |
| Direct tail calls | `codex/tailrec` | `ac5390a` |

All four are staged in `.worktrees/codex-integration`, branch
`codex/integration`. They are not yet accepted into main. The original three
were recovered rather than reimplemented. Review found additional defects:

- Memo hits failed to replay companion receiver routes when two companions
  inherited the same implicit member. The internal regression failed before
  `9fd8bb5` and passed afterwards; this is not yet a demonstrated source-level
  wrong-output example.
- Inserted generic `apply` inference skipped type-argument bounds and the
  receiver substitution. `e3c7ce7` fixes reproduced invalid upper/lower-bound
  acceptance and adds scalac comparisons.
- Null erasure also needed value adaptation, checked casts, actual Object
  arrays, and bottom-array overload collision checks. `83be6e6` fixes the
  reproduced verifier and erasure defects.
- Tail calls now loop, including deep by-name forwarding. Value-class
  extension methods remain explicitly unsupported; do not call the phase
  complete. Integration exposed two by-name typing regressions in
  `cats.Monad.untilM` / `untilM_`. The follow-up now unwraps the argument
  value type for overload scoring while retaining the thunk for codegen.
  Independent focused tests and a fresh cats measurement passed.

## Independent evidence so far

The coordinator ran the new Null/cats tests and the existing trait
interoperability tests on an independent integration checkout. Those passed.
After adding memoization, the three focused suites passed together. With all
four changes and JDK 17, the engine suite (24 tests) and tail-call suite
(4 tests) passed.

At integration checkpoint `dabd59c`, with Temurin 17 selected explicitly:

| Measure | Errors | Files with errors | Classes |
| --- | ---: | ---: | ---: |
| Slick | 0 | 0 | 1490 |
| Cats | 352 | 82 | 0 |
| Gitbucket | 912 | 111 | 0 |
| Scala library | 1625 | 171 | 0 |

These are intermediate measurements, not merge approval. The earlier
three-slice checkout reported cats 350/81; the two added diagnostics are
the by-name regression above. Raw logs and commit/timing records are under
`/tmp/scala-rs-codex/integration/three` and `four-jdk17`.

## Environment and harness defects

The execution environments observed from different worktrees selected
different Java installations. JDK 17 macro-engine cache files (class version
61) were loaded by JDK 15 (maximum class version 59), causing nine engine-test
failures and 35 additional gitbucket macro diagnostics. Captured stderr is
`/tmp/scala-rs-codex/integration/java-trace/stderr.log`. The source-hash-only
cache and runtime selection are being fixed in `codex/macro-runtime`.

For comparable validation, set both variables for the command, without
changing the user's global environment:

```sh
export JAVA_HOME=/Library/Java/JavaVirtualMachines/temurin-17.jdk/Contents/Home
export PATH="$JAVA_HOME/bin:$PATH"
```

The new harness records compiler exit status and fails abnormal/empty
measurements. JVM validation now reports incomplete loads and delegates JDK
classes to the platform class loader. Previously the null parent hid
`java.sql`; fixing that exposed a verifier failure in
`slick.jdbc.DatabaseUrlDataSource` and an initializer throwing `select Factory`
in `slick.compat.collection.package$`. `codex/verify-sql` is investigating.
Two other incomplete initializations need optional PostgreSQL/Oracle driver
classes; missing optional dependencies are not compiler bugs. Details are in
`/tmp/scala-rs-codex/integration/three/verify-verbose.log`.

Corpus and specialization runners now reject missing worker records and the
wrong corpus revision. A proposed separate-compilation-round bug did not
reproduce in the actual worker and was not changed. Another full corpus run
completed its workers but failed during `sort` on non-UTF-8 reference
diagnostics. Preserve `.part` files and recover with `LC_ALL=C sort`; never
discard those completed compilations or call the incomplete aggregate a pass.

## Resume without duplicate jobs

Per-slice logs and runner scripts are under
`/tmp/scala-rs-codex/{implicitmemo,nullcross,catstail,tailrec}`. The active
coordinator has transferred their process tracking to a Luna validation
agent. Check recorded tool session completion and logs before starting any
replacement run. Initial runs interrupted for a source fix, or failed because
of the Java mismatch, are retained and are not passing workspace evidence.

The user's current preference is Luna / xhigh for subagents. Use explicit
worktrees and small, independent tasks; keep final integration review with the
coordinator. Current outstanding work is to resolve the concrete defects
above, finish branch and integration gates, save a complete per-test corpus
reference, and only then update the baseline and move main.

## Later coordinator checkpoint

Candidate `b1346e9` is frozen in `codex/integration` for independent full
validation. The tracked command was exec session `81028`, running
`/tmp/scala-rs-codex/integration/validate-candidate.py`; outputs are under
`/tmp/scala-rs-codex/integration/candidate-b1346e9`. It starts with release
workspace tests and stops if they fail. Later phases cover the four measures,
strict verification with the pinned PostgreSQL/Oracle dependencies, Slick
execution modes b and a, specialization, and the complete corpus. Do not
restart this runner. It ended with exit 1 after the workspace command failed
with exit 101: two `final3` tests expose inconsistent stack maps in a tail-call
loop (`Main$.fix`, Object versus Option in local 2). No later phase ran.
The remaining previously unrun CLI targets and other workspace tests completed
with exit 0 on the same frozen candidate (session `29423`, 529.206 and 17.245
seconds). This is failure collection, not a replacement passing workspace
result; the original `final3` failure remains in that candidate.
The existence of `run.json` means the job was dispatched, not that it passed.

Additional independently checked changes staged there:

- Parent constructor defaults: the selected overloaded constructor receives
  its omitted arguments. The minimal program passes JVM verification and
  agrees with scalac (`jdbc:test:user:password`). At `8347b99`, Slick emitted
  1490 classes and the old `DatabaseUrlDataSource` verifier failure was gone.
- By-name overload matching: the combined focused suites passed (6 tests),
  and cats returned to 350 errors / 81 files. Logs are in `integration/recheck`.
- Macro cache/startup: compile the bridge to class version 52 and publish a
  private staging directory atomically. Both tools follow `JAVA_HOME`.
  Independent review rejected the first Java fallback: Java 15 has no
  `Lookup(Class,int)` constructor. The corrected helper passed the same
  proxy invocation on Java 8, 15, and 17; the combined engine suite then passed
  all 27 tests on Java 15. Evidence is in `integration/macro-review` and
  `integration/macro-fixed`. The coordinator also fixed staging cleanup when
  javac cannot start.

The qualified `Factory` companion fix (`607765f`) and generic-owner override
bound substitution (`25a347b`) have been reviewed and staged. An independent
focused run on the next candidate `e4c99ce` passed 31 tests: arraygen 4, codegen
diagnostics 2, override 23, and verify_sql 2 (session `1216`, exit 0). The previous independent strict
Slick check loaded 1489 of 1490 classes, with no verifier failures and exactly
one incomplete initializer (`select Factory`). The Oracle jar used to resolve
optional coverage is the version pinned in Slick's Dependencies.scala:
`com.oracle.database.jdbc.debug:ojdbc8_g:21.23.0.0`.

The isolated tailrec corpus was recovered from completed worker records:
5324 unique records, all six fields present; pos 1052/462/345, neg 660/376/369,
run 572/935/553. This is not an accepted result against the baseline. Its
Java 15 environment and the observed compiler changes need to be separated.
The old four workspace runs all stopped at the nine engine startup failures;
none is a passing full-workspace result. Java 17 reruns of nullcross and
catstail each report 2207 passing tests and no failures. Completion and later
gates are tracked by the validation agent in `slice-validation/result.json`.

Further harness checks now cover non-UTF-8 bytes in both the ledger and its
report, separate compilation rounds, missing worker records, and early
`System.exit(0)` from a class initializer. The latter used to return success
before the sweep completed; an explicit completion record now rejects it.
The measurement harness passes 22 focused checks.

New work remains isolated from the frozen candidate: constructor-default
separate-compilation follow-up, the deferred blocking-slick import fix and its
bounded performance experiment, and codegen diagnostics. The codegen audit
found fourteen fallback sites that emit runtime exceptions for compiler
unsupported/unresolved states. Commit `12098f0`, staged as `e4c99ce` in `codex/integration-next`, makes those
compilation errors with source positions and prevents classfile publication
on emission failure, while preserving deliberate user throws, MatchError,
and `???`. Independent focused checks passed; full gates remain open.

The external constructor-default follow-up `c74dd7b` is held for review: its
manual result typing could hide invalid generic default arguments. Scalac
accepts a separately compiled `GenericDefault[T](value: T = 42)` but rejects
a client extending `GenericDefault[String]()`; this negative example must
remain rejected. The agent is checking this and other binary getter cases.

The deferred gitbucket import fix `b0e4401` is not accepted. Its bounded
120-second measure timed out before diagnostics or class output; zero partial
errors is not a valid error count. A short process profile will distinguish
compiler work from the startup delay observed in another measurement.


## Subsequent independent review

The next candidate `e4c99ce` is frozen for full workspace failure collection
with `--no-fail-fast`, session `87421`; its log is
`/tmp/scala-rs-codex/integration-next/workspace-e4c99ce.log`. The new diagnostic
gate rejects the previously passing `dbio` fixture with `no super
implementation for superZip`. Investigate the generated accessor and target;
do not restore a hidden exception stub merely to make the test green.

Tail-loop frame fix `42acefd` reuses declared local types. Value-class extension
follow-up `bcd9c20` is not yet accepted: independent receiver-changing Long
recursion compiles but fails JVM verification at `astore_0`. Scalac runs the
same program with a small stack and prints `2000007`. Evidence is under
`/tmp/scala-rs-codex/value-tailrec-review`; an additional fix is in progress.

All three original branches have now passed their explicit-JDK-17 workspace
gates: implicitmemo 2204 tests, nullcross and catstail 2207 each. Applying the
current strict verifier to the latter two existing output trees exposes
1490 total / 1488 loaded / 1 verifier failure / 1 incomplete initializer,
corresponding to the constructor and Factory defects already fixed in the
integration candidate. Their corpus gates remain open; the validator must
record harness and binary source revisions separately.

The bounded gitbucket profile captured the compiler at 100% CPU and about
553 MiB RSS, with Rust implicit-search, unification, type substitution and
clone frames, and no `_dyld_start` frame. This particular timeout is compiler
work, not the separately observed startup delay. A semantics-preserving
optimization remains isolated in `codex/gb-import`.


## Recovery candidate separated from new features

To avoid delaying the original three slices behind newly introduced tail-call
and diagnostic work, `codex/recovery` at `d9eb5dc` contains the original three
plus the reviewed parent-constructor, Factory, owner-bound, and Java macro
fixes. It excludes tail-call lowering and the new backend diagnostic gate.
The independent full runner is session `28152`, working in
`.worktrees/codex-recovery`, with logs under
`/tmp/scala-rs-codex/integration/candidate-d9eb5dc`. It is frozen while the
runner proceeds through workspace, four measures, strict JVM verification,
Slick execution, specialization, and corpus. No passing result is implied by
this dispatch record.

The parallel `e4c99ce` workspace run additionally found the same super-target
diagnostic in `genrep`, `lastone`, and `mismatch13`. The coordinator reduced
the issue to `Layer.helper = super.foo`: scalac compiles and prints `base`,
but the new gate searches for a super implementation of `helper`. Evidence is
in `/tmp/scala-rs-codex/codegen-review`. Check actual selected super methods,
not just private-helper exclusion.

Value-class receiver follow-up `f79b75c` now passes the coordinator's original
Long receiver repro, printing `2000007` under full JVM verification and a
small stack. The pending tail-call sequence is `42acefd`, `bcd9c20`, then
`f79b75c`; combined independent gates are still required.

Current process and pending-commit state is also recorded in
`/tmp/scala-rs-codex/integration/current-state.json`. Original branch corpus
sessions and script/binary revisions are tracked separately in
`/tmp/scala-rs-codex/slice-validation/result.json`.


## Variance accepted; other candidates still under repair

`9beb69b2` merges compiler candidate `629570a8` plus test-only `803be601`.
Parent full release validation: 2237 passed, zero failed, 199 result rows;
Slick 0/1490, cats 350/81, GitBucket 912/111, Scala library 1613/171;
strict verification loaded all 1490 classes; Slick MODE=b 36/36 attempts.
MODE=a remains 12 compile failures; specialization remains 2 match / 26 differ /
9 no_compile. All 5324 corpus identities and statuses match the checked-in
three-column reference. Full six-field comparison to `candidate-4b2f941` found
only t8199's output path and impconvtimes's first runtime exception. The latter
was independently investigated: both compilers emit seven byte-identical classes,
and repeated runs produce both VerifyError and IncompatibleClassChangeError.
Evidence is `integration/variance-impconvtimes/results.json` under the existing
`/tmp/scala-rs-codex` root. The full gate, supplemental test and clippy audits
are in `integration/candidate-629570a/`. Compiler/Cargo inputs are unchanged
between the full run, test-only follow-up, and merge. Clippy with the historical
`--workspace --release` scope retains exactly 59 warning messages.

Unaccepted candidates: codegen/constructor `73cc75b2` full workspace returned
2238 passed / 10 failed. Parent `aa5126ee` fixes the six selfrec failures by
indexing default parameter types against the remaining method clauses;
selfrec plus verify_sql gives 10 fresh passes. Four Slick constructor overload
failures remain assigned for repair. Implicit candidate `fc2ab4cc` passes 15
focused tests but rejects a parent-confirmed nsc-positive non-generic implicit
leaf; real GitBucket still timed out at 60 seconds on its preceding candidate.
Macro `7f2a9be9` passes a success-required hydration test, but independent probes
expose incomplete flags, owners, parameter metadata and legal class shapes.
None of these candidates is accepted based on focused tests alone.


## Later independent validation and remaining semantic defects

The accepted baseline remains `9beb69b2`; the following are unaccepted slices.

- Method specialization `5b85263e`: full workspace 2242 passed, zero failed,
  199 result rows; all four compile measures, strict verification of 1490
  classes and Slick MODE=b 36/36 remain unchanged. All 5324 corpus identities
  and statuses agree with the baseline. The six-field comparison against
  `candidate-629570a` differs only in t8199's temporary output path. Evidence:
  `integration/candidate-5b85263/{results.json,corpus-parent-audit.json}` under
  `/tmp/scala-rs-codex`. Acceptance is held for complete traversal of self types,
  type definitions and annotation subtrees, plus one new clippy warning.
  Class-owned specialization remains unimplemented; the nsc-only class oracle
  at `256b9b49` independently passes with output `3:5:su:25:47:sv:3:f` and
  class-specific descriptor/bridge assertions. It is not a scala-rs pass.
- Source signature `9144b8fd`: full workspace 2221 passed, 20 failed, 200 result
  rows, exit 101. Failed targets: buildfrom, cpvalueclass, engine, ifacebridge,
  mismatch11, rf_reify, twirl and xflags. Failures include wrong collection
  result types, value-class extension descriptors, macro argument counts,
  missing primitive boxing and missing Equals.canEqual. No compile measures
  or corpus followed this failed gate. Evidence: `integration/candidate-9144b8f`.
- Codegen/constructors: synthetic `$outer` field metadata resolves the previous
  four Slick constructor failures. A fresh focused run at `dd224fd2` passes
  31 tests including the earlier ten failures and the user-named `$outer`
  negative. Parent `765c6cd3` names the super-accessor ABI record, removes an
  unused tuple component and simplifies constructor-clause traversal. This
  frozen candidate is undergoing full acceptance; do not reuse the old failed
  `73cc75b2` numbers as its result.
- For value guards `02c1465a`: fresh fvg 4 and mismatch6 12 pass. Independent
  runtime probes still fail against nsc 2.13.16: a custom Box.map sees
  `((1,2),3)` instead of `(1,2,3)`; 22 value definitions over two List elements
  execute `21:1;22:1;21:2;22:2;` instead of `21:1;21:2;22:1;22:2;`.
  All four compiler/runtime invocations in each probe exit zero. nsc groups
  at most 21 value definitions and emits a flat TupleN (TreeGen.scala:728).
  Evidence: `integration/for-tuple-shape-probe/results.json` and
  `integration/for-group-boundary-probe/results.json`. Follow-up repair is active.

The process handles in `/tmp/scala-rs-codex/integration/current-state.json`
are hints, not proof of liveness; poll each handle before continuing or restarting.


## Corpus losses and temporary-directory race

Codegen/constructor `765c6cd3` finished its release workspace with 2249 passed,
one failed, 200 result rows. The failure was independently traced to two
`verify_sql` tests receiving the identical timestamp-plus-PID temporary root.
The instrumented log shows the duplicate paths and the wrong test's Main
output: `/tmp/scala-rs-codex/verify-sql-race/repeat-instrumented.log:486`.
One test can delete the other's nsc classpath. Parent isolated rerun passed all
five tests. The process-local atomic sequence fix was applied to main as
`72c41271`; parent main's two existing verify_sql tests pass. Compiler/Cargo
inputs and test counts are unchanged. The original failed workspace result
remains in the evidence; it has not been rewritten as an all-pass run.

Remaining gates for `765c6cd3` independently establish real compiler regressions:
Slick reports 66 errors in 18 files and emits zero classes (62 missing super
implementations for toString, four unsupported sql selections). Its strict
verification and execution gates consequently fail. Cats remains 350/81,
GitBucket reports 893/110 and Scala library remains 1613/171. The corpus has
5324 unique records: pos 1045/469/345, neg 669/367/369, run 588/919/553.
Eleven old passes are lost, one positive changes to pass, and ten negative
cases newly reject. Several new rejections are unsupported-codegen diagnostics,
so the negative count is not a compatibility improvement. Full six-field
comparison changes 52 rows; evidence is `candidate-765c6cd/corpus-detail-audit.json`
and `corpus-status-audit.json`. Separate super-implementation and sql-selection
fixes are active; the candidate remains unaccepted.

For-comprehension `dd86599b` completes workspace with 2242 passed, one failed,
200 result rows. The only failure is conform::for_comprehensions: a String
result reaches MapOps.WithFilter.map and is cast to Tuple2. No later gates ran.
Its prior 18 focused passes and three exact nsc comparisons do not establish
full acceptance. The Map WithFilter repair and private Tuple1..Tuple22 support
are independent pending tasks.

## Core implementation consolidated in the parent session

After repeated integration regressions, core implementation is now owned by
the parent session. Luna/xhigh agents have saved their existing work and stopped
implementation. Any later delegation should be bounded reproduction, test
execution or read-only review. Do not restart the former implementation tasks
from stale summaries. Preserve all worktrees, including uncommitted experiments.

The method-specialization traversal repair `93af4b01` was applied independently
as `de970972`, then merged with main at `11103987` in
`.worktrees/codex-specialization-validation`. All nine focused specialization
tests pass in the parent environment. A read-only review found no remaining
traversal omission in the repaired fields. Parent typed-graph invariant tests
are being added before the next full gate; this candidate is not accepted yet.

Class-specialization candidate `811c24a0` passes 12 focused tests, but independent
interop probes prove incorrect packaged/nested JVM names, a VerifyError in an
accessor unrelated to the specialized type parameter, and an override dispatch
returning the base implementation's answer. Evidence:
`/tmp/scala-rs-codex/integration/class-package-probe/results.json` and
`/tmp/scala-rs-codex/class-specialization-audit/`. It remains unaccepted.

Saved, not independently accepted, follow-ups include:

- `12bec5ce`: terminal Object super accessor resolution.
- `c04acead`: completed case-class macro metadata fidelity.
- `43af3d29`: implicit Function lookup and dominance symbol handling.
- `c1dc7aa8`: prefix-preserving TypeProjection representation.
- `558b009d`: private Tuple1 through Tuple22 runtime.
- `40f1afaa`, `a65123a7`: collection result handling and external generic copy.

The implicit performance experiment at `codex-implicit-profile` is explicitly
not acceptable: its anchorless-child pruning accepts a case where nsc reports
ambiguity. Keep its profile evidence, not its success count, as the next starting
point. The sql-selection worktree contains unproven changes and debug output.
The Map WithFilter patch is preserved in the actual git worktree
`/tmp/scala-rs-codex/for-map-filter-fix-worktree`; its None-result fallback still
needs parent review. The live checkpoint index is
`/tmp/scala-rs-codex/integration/current-state.json`; revalidate tool handles
before treating any recorded process as running.

Parent follow-up: typed-graph tests were committed as `12222dc4` (two pass),
and the fixed method candidate is `e4d404f1`. Its full gate is running under
`candidate-e4d404f`; historical-scope clippy passes with 58 warnings versus 59,
no new warning messages. Do not edit that frozen worktree during the run.

Parent repaired Map WithFilter as `dc6f2176`: the missing result type is no
longer treated as proof of a non-pair overload, and non-pair flatMap results
use the receiver's IterableCC constructor instead of the lambda's List type.
The new fixture checks pair/non-pair map and flatMap, function values, strict
runtime equality, and rejection of an invalid List assignment against nsc.
The fresh focused gate passes 105 tests (conform 86, fvg 7, mismatch6 12).
After merging main, frozen candidate `c6a7e8f9` is in its full gate under
`candidate-c6a7e8f`. Neither candidate is accepted yet. All implementation
subagents have now stopped with their work preserved.


## Parent repair: local template completion and macro declaration stripping

On the held codegen/constructor candidate, inferred macro implementations were
cached during signature inference with untyped local TypeCreator bodies. The
local class/object/anonymous-class expression entry now completes their bodies,
while member and top-level templates retain the two-pass entry. Macro declaration
stripping now follows all child edges, including classes nested below New and
ValDef, rather than only directly nested templates.

Five lost positive corpus cases pass after the first repair; the two lost runtime
cases macro-term-declared-in-anonymous and macro-term-declared-in-refinement pass
after the stripping repair. Evidence is under
`/tmp/scala-rs-codex/integration/codegen-macro-parent-probe/after-local-template/`:
`corpus.tsv` and `strip-corpus.tsv`. Existing engine 27, rf_reify 6 and xflags 10
focused tests pass. The new lazy_template test passes all four compiler producer/
consumer combinations through an nsc reflection adapter, with strict JVM
verification and output 7. The adapter deliberately isolates executable bodies
from the following independently reproduced, still RED ScalaSignature defects:

- Direct scala-rs macro declarations omit MACRO/binding metadata; a scalac client
  compiles an ordinary call and fails at runtime with NoSuchMethodError.
- A direct nsc macro facade rejects the scala-rs implementation result because
  Exprs$Expr[Int] loses its dependent c.Expr[Int] representation. Even a source
  cast cannot work around the incorrectly named nested class type.

The original unadapted tests and failing logs are retained in the evidence
folder as direct-macro-signature-red.rs and dependent-result-signature-red.rs.
The adapted passing test is proof of local body execution, not of the direct
Scala macro-declaration ABI. These fixes have not passed a full acceptance gate.
