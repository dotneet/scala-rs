# Codex continuation: validation in progress

This is a progress record, not a completed compatibility claim. Recovery was
independently measured at `d9eb5dc` and merged into local main as `4b0568af`.
Read the updated `tests/BASELINE.md` rather than remeasuring the old tree.

## Current decision state

The sections below preserve earlier checkpoints; this section supersedes
their pending-status statements. All new subagents use Luna / xhigh and
isolated worktrees. They report through collaboration tools, not Codex task
messages.

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
- New features remain separate in `codex/integration-next` at `6f26485`.
  The tail-call fixes independently pass 28 focused release tests and the
  super-accessor correction passes 101. A fresh parent-built provider plus a
  scalac-built subclass still raises `AbstractMethodError` for inherited
  `Layer$$super$foo`; metadata, overload, and nested-owner fixes are pending.
  An earlier ABI probe accidentally used the agent's stale release binary
  and is explicitly invalidated; only `parent-6f26485-*` evidence is current.
- Gitbucket import completion plus a conservative implicit candidate filter
  is at `3f4e170` in `codex/gb-validation`. Session `18153` completed the
  release workspace suite with 2231 passing tests and one diagnostic-text
  failure in `slickimpl`; gitbucket timing was not run. The changed diagnostic
  still rejects the invariant `BaseTypedType[Any]` assignment, but its type
  provenance must be reviewed before changing the expectation.
- External constructor default getters (`c74dd7b`, `1140fb6`) remain held.
  Generic and nested cases now have focused evidence, but assigning default
  flags to every overloaded constructor from getter names requires review.

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
