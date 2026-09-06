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

| commit | `4b0568af` |
|---|---|
| updated | 2026-09-06 |

Measured independently at `d9eb5dc`, then merged as `4b0568af`; their compiler
sources and Cargo inputs are identical. Later documentation-only commits may
follow this commit. Java is Temurin 17 with both `JAVA_HOME` and `PATH` pinned;
the corpus run inherited `LANG=LC_ALL=LC_CTYPE=C.UTF-8`.
Evidence: `/tmp/scala-rs-codex/integration/candidate-d9eb5dc/results.json` and
`corpus-parent-audit.json`. All required historical gates completed with exit
0; the newly measured MODE=a check is explicitly red below.

## Compile measures

| check | errors | files with errors | classes |
|---|---:|---:|---:|
| `tests/slick_measure.sh` (184 files) | **0** | **0** | **1490** |
| `tests/cats_measure.sh` (339, 1 skipped) | **350** | **81** | — |
| `tests/gitbucket_measure.sh` (353, 1 skipped) | **912** | **111** | — |
| `tests/scalalib_measure.sh` (538) | **1613** | **171** | — |

## Execution

| check | result |
|---|---|
| `MODE=b tests/slick_run.sh` | `progs=12 ok=12 diff=0 fail=0 runs=3 attempts=36/36` |
| `MODE=a tests/slick_run.sh` | **RED**: all 12 client programs fail to compile; no execution attempts |
| `tests/slick_subset.sh` | `subset_files=184 classes=1490 verified=1490 failed=0` |
| `tests/classfile_lint.py` (via subset / slick_run) | `lint_problems=0` |
| `tests/verify_all.sh <slick out>` | `verify_classes=1490 verify_loaded=1490 verify_failures=0 verify_incomplete=0` |

## scala/scala corpus (`CORPUS_SIZE=full`, 5324 units)

| kind | pass | fail | skip |
|---|---:|---:|---:|
| `pos` (1859) | **1053** | 461 | 345 |
| `neg` (1405) | **659** | 377 | 369 |
| `run` (2060) | **587** | 920 | 553 |

The complete per-test status reference is
[`baselines/corpus-4b0568af.tsv`](baselines/corpus-4b0568af.tsv): 5324 unique
records, from scala/scala revision `3f6bdaeafde17d790023cc3f299b81eaaf876ca3`.
Compare by `(kind, test)` as well as totals. The full six-field diagnostic
ledger is `candidate-d9eb5dc/corpus.tsv` in the evidence directory above.

Use `python3 tests/compare_corpus.py tests/baselines/corpus-4b0568af.tsv
<candidate-corpus.tsv>` to compare saved ledgers. It rejects missing or
duplicate identities, lost passes, and newly skipped tests. A zero exit only
checks statuses; changed diagnostics and runtime evidence still need review.

Compared with the independently checked Null slice ledger (which matches the
old baseline totals), only `run/t5629` and `run/t12478` changed to pass.
`t5629` fixes overriding bounds inherited from a generic owner; it does not
prove specialization. `t12478` is an environment correction, not a compiler
gain: the same classfiles print `?` under JDK 17 with `LC_ALL=C`, and the
expected Unicode bytes under the recorded UTF-8 locale. Its three-way locale
probe is in `candidate-d9eb5dc/unicode-audit/results.json`.

## Other

| check | result |
|---|---|
| `cargo test --workspace --release --no-fail-fast` | **195 result rows, 2228 passed, 0 failed** |
| `tests/spec_classfiles.sh` | `tests=37 match=2 differ=26 no_compile=9`, `$sp` scalac=700 scala-rs=0, **LEDGER RED** |

## The six unloadable classes are fixed (2026-09-06)

`tests/verify_all.sh` reported `verify_failures=6` when it was written, and the
failures predated it by an unknown number of waves. `agent/verifyfail` closed
all six; the check now reports **0** and is part of the battery.

Worth keeping in mind rather than filing away: fixing the root behind those six
turned up **four more classes broken the same way that verify cleanly** --
`super.expr(n)` resolved to the class's own `expr`, which is legal bytecode and
merely infinite recursion. **A verifier's failure count is a lower bound on
miscompilation**, not a measure of it.

## What is deliberately red

* MODE=a makes scalac read our source ScalaSignature and macro classfiles.
  Curried/implicit parameter sections are flattened before pickling;
  existential and member metadata gaps are also exposed. MODE=b passing does
  not close these reverse-interoperability obligations. There was no MODE=a
  figure in the previous baseline.
* 5 `neg` tests expect a diagnostic nsc's `specialize` phase issues; the ledger
  above is what says they are still owed. See [`../docs/specialization.md`](../docs/specialization.md).
* 70 `pos` and 33 `run` `@specialized` tests need stage 2 for a pass that means
  what nsc's means; stage 1 already turned the incidental ones green.
* gitbucket rose from 1373 earlier in the session, and that was progress rather
  than a regression: a wrong type had been swallowing 269 errors' worth of call
  sites, and removing it let the query bodies be type-checked for the first
  time. See the `agent/tablequery` merge.
