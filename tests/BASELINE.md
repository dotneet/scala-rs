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

| commit | `9beb69b2` |
|---|---|
| updated | 2026-09-06 |

Measured independently at `629570a8`, then merged as `9beb69b2`. The only
change after the full run was a stronger, portable variance interoperability
test, independently passed at `803be601`; compiler sources and Cargo inputs
are identical. Later documentation-only commits may follow this commit.
Java is Temurin 17 with both `JAVA_HOME` and `PATH` pinned; the corpus run
inherited `LANG=LC_ALL=LC_CTYPE=C.UTF-8`.
Evidence: `/tmp/scala-rs-codex/integration/candidate-629570a/results.json`,
`corpus-parent-audit.json`, `clippy-parent-audit.json`, and
`supplemental-variance.log`. Historical passing gates remain passing;
MODE=a and the specialization ledger remain explicitly red below.

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
| `run` (2060) | **590** | 917 | 553 |

The unchanged complete per-test status reference is
[`baselines/corpus-318c1568.tsv`](baselines/corpus-318c1568.tsv): 5324 unique
records, from scala/scala revision `3f6bdaeafde17d790023cc3f299b81eaaf876ca3`.
Compare by `(kind, test)` as well as totals. The current six-field diagnostic
ledger is `candidate-629570a/corpus.tsv` in the evidence directory above.
All statuses match the preceding baseline. Two six-field records differ from
`candidate-4b2f941`: the temporary path in `run/t8199`, and the first exception
reported for already-failing `run/impconvtimes`. Independent recompilation
with both binaries produced seven byte-identical class files; repeated JVM
runs of those same files produce both VerifyError and IncompatibleClassChangeError.
This is an existing invalid program emission with variable failure order,
not a variance regression or improvement. Evidence:
`/tmp/scala-rs-codex/integration/variance-impconvtimes/results.json`.

Use `python3 tests/compare_corpus.py tests/baselines/corpus-318c1568.tsv
<candidate-corpus.tsv>` to compare saved ledgers. It rejects missing or
duplicate identities, lost passes, and newly skipped tests. A zero exit only
checks statuses; changed diagnostics and runtime evidence still need review.

Compared with the previous recovery baseline (`4b0568af`), all previous
passes are retained and only three statuses improve: `run/t3761-overload-byname`,
`run/t8893`, and `run/t8893b`. The coordinator separately compiled and executed
all three with both scala-rs and scalac 2.13.16; their output bytes agree.
The first fixes by-name overload selection; the latter two no longer overflow
the stack. Apart from these, the six-field ledgers differ only in the output
path embedded in the existing `run/t8199` filename-too-long diagnostic.
Evidence is in `candidate-dd5047e/corpus-detail-audit.json` and
`runtime-parent-audit/results.json`.

The earlier recovery gains remain distinct: `run/t5629` fixes overriding
bounds inherited from a generic owner; `run/t12478` was a UTF-8 locale effect,
not a compiler gain. See the preserved recovery ledger and its
`candidate-d9eb5dc/unicode-audit/results.json`. Do not compare a JDK 17 run
under `LC_ALL=C` with this UTF-8 baseline as if their runtime environments match.

## Other

| check | result |
|---|---|
| `cargo test --workspace --release --no-fail-fast` | **199 result rows, 2237 passed, 0 failed** at `629570a8` |
| Test-only validation at `803be601` | **1 passed, 0 failed** (`--test variance`); strengthens an existing test, so the total remains **2237 tests / 199 result rows** |
| `tests/spec_classfiles.sh` | `tests=37 match=2 differ=26 no_compile=9`, `$sp` scalac=700 scala-rs=0, **LEDGER RED** |

No compiler source or Cargo input changed after the full run. The supplemental
variance test checks class, method, and higher-kinded variance through a real
scalac consumer, validates each invalid assignment's diagnostic location, and
compares actual JVM output. `cargo clippy --workspace --release` exits 0 with
59 warning messages, exactly the preceding baseline's multiset. A separate
`--all-targets` run exposes seven additional warnings in unchanged tests; it
must not be compared to the narrower historical warning count.

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
