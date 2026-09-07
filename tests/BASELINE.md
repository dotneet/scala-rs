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

| commit | `15bee7f9` |
|---|---|
| updated | 2026-09-07 |

Six slices have merged in two composed gates. The first three, at `9739388f`:
`agent/gbtrait` (`new T()` for a trait read from a class file), `agent/catseta`
(the type parameters eta-expansion and inserted applies left open), and
`agent/implicitfilter` (a plausibility pre-filter and a `pinned` correction in
implicit search). The next three, measured together at `15bee7f9`:
`agent/defaultargs` (five roots under default arguments, including a
class-file's defaults being invisible and an anonymous class's `$outer` walk
that emitted a `ClassCastException` regardless of defaults), `agent/backendtypes`
(a deferred type-member declaration outranking the definition that fixes it, and
a `p#T` prefix dropped by the pickle reader), and `agent/overspec` (overload
specificity, plus nsc's shape-type pre-selection, plus a function-typed parent
that `class_reaches` could not see through). The coordinator measured the merged
tree, not the branches.
Every number below was measured after the last source commit; only this file
and the saved corpus ledger changed afterwards. Java is Temurin 17 with
`JAVA_HOME` and `PATH` pinned and `LANG=LC_ALL=LC_CTYPE=C.UTF-8`.

`agent/implicitfilter` is measurement-neutral on all four compile measures by
design: it exists to make the `import_wildcard` `pickle_readable` guard
affordable, and that guard is **still off**. With the pre-filter the 191-file
gitbucket reproduction went from over 600 s to 14.4 s and the full 353-file
measure to 6.3 s, so timing is no longer what blocks the guard. Turning it on
now closes the `Query` family (−170) and loses +194 to the
`BasicBackend#Session` as-seen-from root, which is a separate slice's work.
See `docs/gitbucket.md`.

`agent/catseta` costs gitbucket **+4**, reported rather than hidden: with
`acc: Map[A, Set[A]]`, `acc.getOrElse(e._1, Set())` now infers `Set[_ <: A]`
where nsc infers `Set[A]`, and the newly correct invariant-wildcard rule then
rejects it. The rule matches nsc; the imprecision is upstream of it, in the
order arguments are typed against an undetermined parameter. `docs/cats.md`
carries the analysis.
All previously passing status gates remain passing. MODE=a and class-owned
specialization remain explicitly red; this is not a completion claim.

## Compile measures

| check | errors | files with errors | classes |
|---|---:|---:|---:|
| `tests/slick_measure.sh` (184 files) | **0** | **0** | **1490** |
| `tests/cats_measure.sh` (339, 1 skipped) | **303** | **78** | — |
| `tests/gitbucket_measure.sh` (353, 1 skipped) | **785** | **103** | — |
| `tests/scalalib_measure.sh` (538) | **1553** | **168** | — |

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
| `pos` (1859) | **1073** | 441 | 345 |
| `neg` (1405) | **668** | 368 | 369 |
| `run` (2060) | **616** | 891 | 553 |

The complete per-test status reference is
[`baselines/corpus-15bee7f9.tsv`](baselines/corpus-15bee7f9.tsv): 5324 unique
records from scala/scala revision `3f6bdaeafde17d790023cc3f299b81eaaf876ca3`.
Compared with `0d200adb`, `losses=0` and one status improved (`pos/t6666d`).
Compared with `9739388f`, `losses=0` and six improved: `pos/existential-function-pt`
(the Function/PartialFunction choice `agent/overspec` fixed), `pos/t2809`,
`pos/t4036`, `pos/t9014` (default arguments), `run/reflection-enclosed-basic`
and `run/t9182` (type members). Nothing else moved in either comparison.
Compared with `2098c6fe`, all 5324 statuses are unchanged. The two changed
six-field records are an output path in `run/t8199` and the first reported JVM
exception in `run/impconvtimes`; all seven generated classes of the latter are
byte-identical to the preceding main. Neither is a new success.
Compared with `e12cbdb2`, there are 46 improved statuses (15 pos, 8 neg, 23 run),
no lost passes and no new skips. The 100 changed six-field records were reviewed;
changes in still-failing tests are not counted as successes. In particular,
`neg/t11866` now rejects all four ambiguous calls at the expected lines instead
of rejecting only one via an unrelated bound error.

The 23 newly passing run cases were separately compiled and executed with
scalac and scala-rs under `java -Xverify:all`; outputs match. Evidence:
`gain-audit-eb02d12/results.json` (22 cases) and `t2849-audit/results.json`
(original case plus observable sorted-set contents). Additional positive probes
are in `gain-audit-e521f47`, `corpus-gain-audit`, and
`positive-gain-audit-10f3ef0`. A pos pass proves compilation, not execution:
`pos/t6976` exposed missing static main forwarders on its second compilation.
That separate defect is now fixed in this main baseline, with all four
nsc/scala-rs producer/consumer combinations tested. Some negative gains still have imprecise diagnostics; status
acceptance does not establish exact scalac diagnostic compatibility.

Use `python3 tests/compare_corpus.py tests/baselines/corpus-15bee7f9.tsv
<candidate-corpus.tsv>` to compare saved ledgers. It rejects missing or
duplicate identities, lost passes, and newly skipped tests. A zero exit only
checks statuses; changed diagnostics and runtime evidence still need review.

### Earlier accepted audits

The earlier tail-call/by-name merge, compared with recovery (`4b0568af`),
retained all passes and improved three statuses: `run/t3761-overload-byname`,
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
| `cargo test --workspace --release --no-fail-fast` | **225 result rows, 2312 passed, 0 failed** at `15bee7f9` |
| `tests/spec_classfiles.sh` | `tests=37 match=2 differ=26 no_compile=9`, `$sp` scalac=700 scala-rs=0, **LEDGER RED** |

No compiler source, Cargo input, or test changed after the full run.
`cargo clippy --workspace --release` exits 0 with 57 warning messages versus
57 in the preceding baseline, with no added warning messages. Count the
messages, not `^warning:` lines: the six per-crate "generated N warnings"
summaries make the raw count 63, and a slice reported that as drift. Compare the same command scope;
`--all-targets` also includes historical warnings from tests.

## The six unloadable classes are fixed (2026-09-06)

`tests/verify_all.sh` reported `verify_failures=6` when it was written, and the
failures predated it by an unknown number of waves. `agent/verifyfail` closed
all six; the check now reports **0** and is part of the battery.

Worth keeping in mind rather than filing away: fixing the root behind those six
turned up **four more classes broken the same way that verify cleanly** --
`super.expr(n)` resolved to the class's own `expr`, which is legal bytecode and
merely infinite recursion. **A verifier's failure count is a lower bound on
miscompilation**, not a measure of it.

## The `import_wildcard` guard is ready and is not in yet (2026-09-07)

`docs/gitbucket.md` has blocked the `pickle_readable` guard on
`Typer::import_wildcard` for several waves, for two reasons that are now both
gone: `agent/implicitfilter` removed the 50x cost, and `agent/backendtypes`
fixed the `BasicBackend#Session` root that the guard used to trip over. The
coordinator applied it to this tree as a two-line experiment and measured
**gitbucket 785 -> 717 in 11.6 s** with cats, slick and the library measure all
unchanged; the whole `value list / delete / insert / firstOption / update /
returning is not a member of Query[...]` family (151 errors) disappears and 52
new `type mismatch` errors appear behind it, from code that now types far
enough to fail later. The experiment was reverted; `agent/implguard` is landing
it with a test that actually distinguishes the two binaries and an audit of the
52.

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
