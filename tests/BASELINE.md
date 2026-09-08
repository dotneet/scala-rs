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

| commit | `f4b829ec` |
|---|---|
| updated | 2026-09-08 |

**Twenty-five slices have merged this session**, in eight composed gates. From
this gate on, run them with **`tests/verify_merge.sh`**: one command, one log
directory, one `VERDICT=` line, one `DONE` sentinel, and every skipped step
named in the summary. This one reports `VERDICT=PASS`. The
coordinator measured the merged tree each time, not the branches.

| gate | slices | gitbucket | cats |
|---|---|---:|---:|
| `9739388f` | `gbtrait`, `catseta`, `implicitfilter` | 895 -> 867 | 346 -> 326 |
| `15bee7f9` | `defaultargs`, `backendtypes`, `overspec` | -> 785 | -> 303 |
| `d056a7f7` | `implguard` | -> 717 | 303 |
| `9a00edce` | `projection`, `selftype`, `hkpath` | 717 | -> 251 |
| `7aa47c29` | `linterm`, `gbhead`, `hkselfalias`, `macromirror`, `qualfallback`, `subtypeterm`, `catsinfer`, `linorder2` | -> 398 | -> 231 |
| `8d4cdde0` | `hkunify`, `convimpl` | -> 393 | -> 215 |
| `54df4d43` | `gbmapto`, `basetypeseq`, `catstail`, `gbshape` | -> 337 | -> 196 |
| `f4b829ec` | `mapto2`, `impprio`, `sortedmap`, `gbopt` | -> 270 | -> 188 |

Four of those fifteen move no number and are the most important. **`linterm`
and `subtypeterm` fixed non-termination**: `lin` and `is_sub_type` were bounded
by depth (64 and 200) and *a depth bound bounds the depth of the recursion
tree, not its size*, so a cyclic — or merely wide and legal — hierarchy did
`branching^depth` work. A 41-file cats input ran for **five and a half hours**
and now takes 1.08 s; a legal 26-level diamond took 74 s against scalac's 1.9 s.
Before `linterm`, `trait A extends B; trait B extends A` **compiled to class
files with no diagnostic at all**. **`qualfallback`** closed a silent
wrong-answer: a parent type whose qualifier named nothing fell back on the bare
simple name, so `Missing.AllOps` bound the enclosing trait and a program
compiled with the wrong parent. **`linorder2`** replaced the C3 merge with
SLS 5.1.2's `+:` fold — SLS specifies no C3 merge, and `+:` is total where the
merge had to guess.

`agent/macromirror` built the reverse-RPC channel and `c.typecheck` and
measured a **yield of zero**, with the reason itemised: not one corpus test
reaches a `c.typecheck` call, and `mapTo`'s 31 gitbucket refusals stop one
layer earlier, in `tag_descriptor`, which cannot carry type arguments. That is
where the next slice on macros starts.

`agent/linorder2` also **implemented, measured and retracted** its second half:
walking the linearization in `super_select_member` gives scalac's chain and
costs 5 new library errors, because the reversed-parent walk was compensating
for two deeper defects. Both are named in `docs/not-implemented.md`, in the
order they must be fixed.

`agent/hkunify` (Fable 5.1) implemented nsc's partial unification —
`TypeVar.unifyFull`, scala/bug#2712: with the variable applied to `k`
arguments and the type to `n >= k`, the leftmost `n - k` are captured as
constants and only the rightmost `k` are abstracted, so `Either[String, Int]`
against `F[A]` is `F := Either[String, *]` and never `[x]Either[x, Int]`. No
type lambda had to be invented: a `Type::Class` with fewer arguments than the
class has parameters already *is* the curried constructor here. Its first full
corpus showed `losses=3`, ill-kinded solutions the old code let through; the
parent-walk half of nsc's rule closed them.

`agent/convimpl` then repaired what that exposed. A view whose implicit
argument has no witness must be **discarded during search**, not reported: nsc
answers `value flatMap is not a member of Bag[Int]`. The guard for this already
existed but sat on the widened path only and ran under `&self`, so it could not
load the class file a witness lives in. Making search and fill agree removed a
duplicated diagnostic structurally rather than by de-duplicating messages, and
a case where two views tie on argument type and differ only in their implicit
clauses now compiles as scalac compiles it.

The seventh gate's four slices each corrected the brief they were given, and
three of the four found an "accepting too much" defect on the way:

* **`agent/basetypeseq`** — the culprit was `subst_as_seen_from`, not
  `base_type_seq`: it walks parents in written order and a `seen` set stops the
  second arrival, so the *first-written* parent's instantiation won. Reordering
  the walk measured **1551 -> 1579**; recording, per base class, only the
  instantiation the most derived arrival supplies measured **1551 -> 1420**,
  26 files improved and none worse. The corpus did not move a single line —
  this hierarchy shape does not reach its `run` population, so the -131 is an
  error count and not a test count.
* **`agent/catstail`** — clustering by *which member selection* rather than by
  message shape showed that 15 lines counted as three families were one root:
  a mixin forwarder read from a class file was being treated as a declaration.
  A JVM generic signature cannot write `[B >: A]` and has only one parameter
  list, so `IterableOnceOps.reduceLeft[B >: A]` arrives as
  `<B> B reduceLeft(Function2<B, A, B>)` beside the pickled declaration, and
  overload resolution had two candidates where nsc has one. Before the fix
  `xs.foldLeft(0, f)` **compiled**; scalac rejects it. Two looser versions were
  measured and discarded (slick 0 -> 7 errors; 18 slick class files changed).
* **`agent/gbshape`** — the four gitbucket families were **three** roots. The
  large one: `sig_rerun_safe` only rebuilds a signature that failed, and only a
  rebuild refreshes the scope snapshot, so a member whose own signature was
  fine kept the snapshot from before `import profile.api._` resolved — measured
  at 43 implicits in scope with `columnExtensionMethods` absent. Separately,
  `is_assignment_op` was missing nsc's guards, so `var b = true; b === false`
  **compiled** as `b = (b != false)`. That fix costs +3 on its own and is in
  for soundness, not for the count.
* **`agent/gbmapto`** — the tag descriptor now carries type arguments, and the
  measurement corrected the standing story: nsc's `macroArgs` wraps every value
  argument as `Expr[Nothing]`, so we had been building tags nsc never builds.
  `mapTo` stays refused, with the refusal naming the wall that is actually next
  (the type arguments written on the macro implementation reference, which
  `PickleReader::macro_impl_of` discards).

The eighth gate's four slices again each corrected the brief they were given,
and three of the four found a program we accepted and scalac rejects:

* **`agent/gbopt`** — of the five gitbucket families it was handed, two were
  **not roots**: `OptionMapper2` went 13 -> 0 and `CanBeQueryCondition` 13 -> 2
  without a line written about either. The three real roots share one shape:
  *a class file is a lossy description of someone else's code, and the lossy
  answer gets in first and stays.* A mixin forwarder flattens the parameter
  lists and erases a `Boolean` type argument to `Object`, and it was hiding the
  trait's own declaration — so `7.tagIn(List(1), witness)`, an implicit passed
  positionally, **compiled**. `===` survived only because its JVM name is
  `$eq$eq$eq` and `fill_java_members` does not decode it: operator names were
  intact and alphabetic ones were not.
* **`agent/impprio`** — SLS 2's four-level precedence, not a tie-break. The
  pre-fix compiler resolved `Database()` to a wildcard import over an explicit
  one and **printed `wild` where scalac prints `explicit`**; the negative
  fixture emitted 52 class files with no error where scalac reports three. Its
  first corpus run was `losses=3` and rewrote two of its own rules, after which
  `neg/import-precedence` — scala/scala's own test for this rule — passes.
* **`agent/sortedmap`** — built out both surviving variants of the previous
  slice's three, then **abandoned both**: reordering makes the more derived
  declaration win everywhere, and `MapOps.map[K2,V2]` is more derived than
  `IterableOps.map[B]` *without overriding it*, so `Map("a" -> 1).map { case
  (_, v) => v }` compiled and threw `ClassCastException` at run time. The
  workspace suite and the corpus caught it; the error counts did not. What
  landed instead is one narrow rule about an inherited declaration that adds an
  implicit clause.
* **`agent/mapto2`** — nsc resolves the type arguments written on the macro
  *implementation reference*, and the fingerprint it pickles is an index into
  that list. We discarded them in three places, so when the counts happened to
  match the tags went over in call-site order: `swapped[Int, String]` printed
  `R=Int U=String` where scalac prints `R=String U=Int`. All 31 gitbucket
  `mapTo` sites now invoke `mapToImpl` for real and stop at the next wall.

`tests/verify_merge.sh` earned itself this gate: `agent/gbopt`'s first run
returned `VERDICT=FAIL` on two workspace tests and two corpus `run` tests that
none of its own measures or focused suites reached.

Every number below was measured after the last source commit; only this file
and the saved corpus ledger changed afterwards. Java is Temurin 17 with
`JAVA_HOME` and `PATH` pinned and `LANG=LC_ALL=LC_CTYPE=C.UTF-8`.

`agent/implicitfilter` was measurement-neutral on all four compile measures by
design: it existed to make the `import_wildcard` `pickle_readable` guard
affordable. **That guard is on now** (`agent/implguard`, the section below),
and it is what takes gitbucket from 785 to 717. With the pre-filter the
191-file gitbucket reproduction went from over 600 s to 14.4 s; the full
353-file measure runs in **5.3 s with the guard on**.

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
| `tests/cats_measure.sh` (339, 1 skipped) | **188** | **72** | — |
| `tests/gitbucket_measure.sh` (353, 1 skipped) | **270** | **79** | — |
| `tests/scalalib_measure.sh` (538) | **1420** | **166** | — |

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
| `pos` (1859) | **1088** | 426 | 345 |
| `neg` (1405) | **673** | 363 | 369 |
| `run` (2060) | **618** | 889 | 553 |

The complete per-test status reference is
[`baselines/corpus-f4b829ec.tsv`](baselines/corpus-f4b829ec.tsv): 5324 unique
records from scala/scala revision `3f6bdaeafde17d790023cc3f299b81eaaf876ca3`.
Compared with `7aa47c29`, `losses=0` and **nine statuses improved, nothing
else moved** — `pos/t2712-{1,3,4,7}`, `neg/t2712-2`, `pos/hk-infer`,
`pos/t5683`, `pos/tcpoly_infer_implicit_tuple_wrapper` and `pos/fun_undo_eta`,
which are scala/bug#2712's own partial-unification tests and are the direct
evidence that `agent/hkunify` implemented nsc's rule rather than a rule that
happens to fit cats. Compared with `d056a7f7`, `losses=0` and **eight
statuses improved**: `neg/t7507` (a `self: Cake =>` seeing `Cake`'s `private[this]`,
which we used to accept), `pos/t10714`, `pos/t10714b`, `pos/t7753` (dependent
result types through an inserted `apply`), `pos/t6895` (partial expected-type
solutions), `pos/t8801`, `run/t102` and `run/t3798`. Every earlier gate was
`losses=0` as well; across the whole session no corpus status has ever gone
from pass to fail.
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

Use `python3 tests/compare_corpus.py tests/baselines/corpus-f4b829ec.tsv
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
| `cargo test --workspace --release --no-fail-fast` | **247 result rows, 2407 passed, 0 failed** at `f4b829ec` |
| `tests/spec_classfiles.sh` | `tests=37 match=2 differ=26 no_compile=9`, `$sp` scalac=700 scala-rs=0, **LEDGER RED** |

No compiler source, Cargo input, or test changed after the full run.
`cargo clippy --workspace --release` exits 0 with **58** warning messages,
counted as messages and not as `^warning:` lines. The figure stood at 57 for
several gates and was stale: a slice reported 58, and checking out the
recorded baseline commit into a separate worktree and running the same
command there gave 58 as well. Count the
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

## The `import_wildcard` guard landed (`agent/implguard`, 2026-09-07)

`docs/gitbucket.md` had blocked the `pickle_readable` guard on
`Typer::import_wildcard` for several waves, for two reasons that are both gone:
`agent/implicitfilter` removed the 50x cost, and `agent/backendtypes` fixed the
`BasicBackend#Session` root that the guard used to trip over.

Measured on `agent/implguard` after merging `main` at `15bee7f9`:
**gitbucket 785 -> 717 errors, `files_with_errors` unchanged at 103, in 5.3 s**,
with cats (303/78), slick (0/0, 1490 classes) and the scala library (1553/168)
bit-for-bit unchanged. The whole `value list / delete / insert / firstOption /
update is not a member of Query[...]` family — 205 errors on this tree — goes
to zero; 52 net new `type mismatch` errors appear behind it, from code that now
types far enough to fail later.

The 52 were audited, not assumed: every one is a query result whose element
type is `Any`, because `TableQuery[E] extends Query[E, E#TableElementType, Seq]`
and that projection is still not computed — the same `Any` the *old* messages
already printed (`value list is not a member of Query[Issues, Any, Seq]`). One
file that was clean gains one error (`IssueCreationService.scala:41`), and it
is a cascade through an inferred result type that used to be `None.type`
because the query body had collapsed to `Nothing`. Nothing that used to be
right became wrong. `docs/gitbucket.md`, *What landed, and what it uncovered*.

The test is `tests/multi/implicit_wildcard_binary` + `crates/cli/tests/implguard.rs`:
real scalac compiles the library, scala-rs the consumer, and the fixture was
verified to **fail on the unguarded binary and pass on the guarded one**. The
ordering is load-bearing — an `object api { implicit def … }` reached by a
wildcard import compiles either way and proves nothing.

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
