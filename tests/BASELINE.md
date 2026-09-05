# Current numbers on `main`

**Read this instead of measuring the baseline yourself.** Every slice used to
spend twenty to thirty minutes establishing "before" figures that the previous
slice had already established — six times over in a six-slice wave. The
coordinator updates this file on every merge, from the verification run that
gated that merge.

Measure *your* tree, compare against this, and report both. If a number here
disagrees with what you measure on an unmodified tree, **stop and report** —
that means either this file is stale or your branch is not where you think it
is, and both invalidate everything downstream.

| commit | `c451517` |
|---|---|
| updated | 2026-09-05 |

## Compile measures

| check | errors | files with errors | classes |
|---|---:|---:|---:|
| `tests/slick_measure.sh` (184 files) | **0** | **0** | **1490** |
| `tests/cats_measure.sh` (339, 1 skipped) | **474** | **88** | — |
| `tests/gitbucket_measure.sh` (353, 1 skipped) | **912** | **111** | — |
| `tests/scalalib_measure.sh` (538) | **1647** | **171** | — |

## Execution

| check | result |
|---|---|
| `tests/slick_run.sh` | `progs=12 ok=12 diff=0 fail=0 runs=3 attempts=36/36` |
| `tests/slick_subset.sh` | `subset_files=184 classes=1490 verified=1490 failed=0` |
| `tests/classfile_lint.py` (via subset / slick_run) | `lint_problems=0` |
| `tests/verify_all.sh <slick out>` | `verify_classes=1490` **`verify_failures=6`** — see below |

## scala/scala corpus (`CORPUS_SIZE=full`, 5324 units)

| kind | pass | fail | skip |
|---|---:|---:|---:|
| `pos` (1859) | **1053** | 461 | 345 |
| `neg` (1405) | **659** | 377 | 369 |
| `run` (2060) | **575** | 932 | 553 |

## Other

| check | result |
|---|---|
| `cargo test --workspace --release` | **188 binaries, 2176 passed, 0 failed** |
| `tests/spec_classfiles.sh` | `tests=37 match=2 differ=26 no_compile=9`, `$sp` scalac=700 scala-rs=0, **LEDGER RED** |

## Six classes the JVM will not load

`tests/verify_all.sh` reports `verify_failures=6` on slick's output, and has
since it was written (2026-09-06); the failures predate it by an unknown number
of waves. **This is a real defect, not an accepted limitation.**

```
slick.collection.heterogeneous.HList$                   Bad type on operand stack
slick.jdbc.PositionedResult$$anon$507                   Bad type on operand stack
slick.jdbc.PostgresProfile$PostgresQueryBuilder         Bad invokespecial instruction
slick.memory.DistributedProfile                         Bad return type
slick.memory.MemoryProfile$InsertMappingCompiler$…      Bad type on operand stack
slick.memory.MemoryQueryingProfile$MemoryCodeGen$…      Bad type on operand stack
```

`slick_measure.sh` reporting `errors=0` and this line are both true and are
different claims. See [`../docs/testing.md`](../docs/testing.md) for why every
other check missed them.

## What is deliberately red

* 5 `neg` tests expect a diagnostic nsc's `specialize` phase issues; the ledger
  above is what says they are still owed. See [`../docs/specialization.md`](../docs/specialization.md).
* 70 `pos` and 33 `run` `@specialized` tests need stage 2 for a pass that means
  what nsc's means; stage 1 already turned the incidental ones green.
* gitbucket rose from 1373 earlier in the session, and that was progress rather
  than a regression: a wrong type had been swallowing 269 errors' worth of call
  sites, and removing it let the query bodies be type-checked for the first
  time. See the `agent/tablequery` merge.
