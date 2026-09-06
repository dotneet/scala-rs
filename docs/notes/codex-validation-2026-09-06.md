# Codex continuation: validation in progress

This is a progress record, not a completed compatibility claim or a new
baseline. The implementation on `main` remains the implementation measured at
`902da04`; subsequent main commits currently change documentation and test
harnesses only. Read `tests/BASELINE.md` rather than remeasuring the old tree.

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
  `cats.Monad.untilM` / `untilM_`; `codex/byname-followup` is investigating.

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
