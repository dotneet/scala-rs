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

| commit | `9f3cae13` |
|---|---|
| updated | 2026-09-09 |

**Seventy-three slices have merged this session**, in twenty-nine accepted composed gates.
Five intermediate candidates were rejected, two despite a PASS script verdict. From
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
| `b15df464` | `anyconstr`, `libmaxmin` | 270 | -> 185 |
| `4d613d25` | `libcaseeq`, `libanyval` | 270 | 185 |
| `3fd80269` | `libtailrec`, `libprelude`, `libnotype`, `neglit`, `caseabi`, `pickleparams`, `liboverload`, `negchecks`, `sysout`, `accessmsg` | 270 | 185 |
| `70f349ea` | `arrayelem`, `basetypemeet`, `pkgobjdup`, `siblingover`, `unitpop`, `secondaryctor` | -> 270 | -> 182 |
| `5108669a` | `triemapjava`, `nameamb`, `ctorgaps`, `implctx` | 270 -> 271 | 182 |
| `462aeebb` | `overscore`, `varargsrecv` | 271 -> 270 | 182 |
| `d39d5a65` | `javavarargs` | -> **265** | 182 |
| `aa38b9e9` | `tuplepat` | 265 | 182 -> **177** |
| `d0c89fc1` | `ctorgaps2` | 265 | 177 |
| `80c660c0` | `hkfield` | 265 | 177 |
| `a723e8b1` | `lowerbound` | 265 | 177 -> **168** |
| `e0212fb7` | `basetypeargs` | 265 | 168 -> **167** |
| `b83e06a8` | `preludelb` | 265 -> **264** | 167 -> **166** |
| `56b81c21` | `samconv` | 264 | 166 -> **163** |
| `7b4673c5` | `gbslickmember` | 264 -> **242** | 163 |
| `ef33b16f` | `strarrayops` | 242 | 163 |
| `f428def6` | `varassign` | 242 | 163 |
| `4cc87fb2` | `unqualname` | 242 | 163 |
| `0828f77b` | `wildcard-receiver` | 242 -> **239** | 163 |
| `9f3cae13` | `returning-family`, collection overload corrections | 239 -> **228** | 163 |

Four of those slices move no number and are the most important. **`linterm`
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

The ninth gate turned on `tests/scalalib_measure.sh`, which had had almost no
attention: **1554 -> 1173 errors, 168 -> 157 files**, in two slices.

* **`agent/libmaxmin`** (-194) — the brief's hypothesis was wrong twice over.
  The measure runs `--no-scala-library`, so no jar copy of `RichInt` exists to
  collide with, and `src/library` is not needed to reproduce: fifteen lines do.
  The real cause is that `Predef._` was a **snapshot of members taken when the
  prelude was installed**, not an import, so a run that defines `scala.Predef`
  from source used a copy of a `Predef` the program does not have. It is the
  unfixed half of the defect `agent/preludeshadow` closed for `scala._`. Three
  ways of getting the fix wrong were each measured before being discarded, and
  the third — recording the import rather than its members — type-checked,
  passed the JVM verifier and threw `ClassCastException` at run time, which is
  why that fixture executes instead of compiling.
* **`agent/anyconstr`** (-53) — `type AnyConstr[X] = Any` is a type constructor
  whose body ignores its parameter, so every one-parameter constructor conforms
  to it. Implemented as nsc's general rule (`normalize` + `sameLength`), reusing
  `agent/hkunify`'s representation rather than adding one: one 65-line function.
  It also closed a silent wrong answer, an overload picking `sel(x: Any)` over
  `sel(x: Ops[Int, AnyConstr, _])`.

`agent/libmaxmin` also measured its own **+28 regression** rather than
attributing it away: `value -> is not a member` is a pre-existing defect,
reproducible in twelve lines with no `Predef` involved, and two candidate fixes
for it measured *exactly zero* effect before being dropped. Those lines used to
die one error earlier.

**`agent/anyconstr` found a defect in this gate itself.** `verify_merge.sh`
chose its corpus ledger with `ls -t`, and in a git worktree every baseline
carries the checkout time, so the slice's gate compared against a 106 KB
ledger from eleven gates ago and still printed `VERDICT=PASS`. The ledger is
now read from the name recorded in this file, and one that cannot be resolved
is a `FAIL`. The `gate:` line prints which ledger was used.

The tenth gate finished the library wave: **1554 -> 1111 errors, 168 -> 155
files** across four slices, and both of this gate's two corrected the brief
they were given.

* **`agent/libcaseeq`** — the backend always emitted `canEqual`; the *typer
  symbol* was missing. It is invisible in jar mode because `scala.Equals`
  arrives from a pickle and `override_check::modifiers_are_known` suppresses
  its modifiers, so the deferred member reads as implemented; only a run that
  compiles `Equals` from source exposes it. The audit it was asked for was
  worth more than the 28: against scalac's `-Xprint:typer`, three further gaps
  are now recorded in `docs/comparison-with-scalac.md` — the companion's
  `writeReplace`, the class-side static forwarders, and **a case class's
  `hashCode`, which is a 31-fold where nsc uses MurmurHash3** and so returns a
  different number at run time. 333 slick class files change, in the
  `ScalaSignature` only; every one was decoded and shown to have gained
  `canEqual` and lost nothing, and `javap -p -c` is identical for all 1490.
* **`agent/libanyval`** — **only 1 of the 34 errors was about the top of the
  hierarchy.** The other 33 were `override_check::same_type` answering "same
  type" for any pair mentioning a type parameter, so `MapOps.concat[V2]` read
  as *overriding* `IterableOps.concat[B]` instead of overloading it. Nothing
  was held out: `Any.scala` and its neighbours live in `src/library-aux`,
  which the measure never compiled. Its corpus gains include `neg/t3854`, a
  program we used to accept. Three of its own attempts regressed cats (188 ->
  197) or gitbucket (270 -> 285) where the library count alone showed an
  improvement — the other two measures are what caught them. It also stopped
  the backend bridging an overload, which had been emitting a `checkcast` and
  throwing `ClassCastException` where scalac emits a mixin forwarder.

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
| `tests/cats_measure.sh` (339, 1 skipped) | **163** | **62** | — |
| `tests/gitbucket_measure.sh` (353, 1 skipped) | **228** | **69** | — |
| `tests/scalalib_measure.sh` (538) | **541** | **123** | — |

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
| `pos` (1859) | **1109** | 405 | 345 |
| `neg` (1405) | **690** | 346 | 369 |
| `run` (2060) | **635** | 872 | 553 |

The complete per-test status reference is
[`baselines/corpus-9f3cae13.tsv`](baselines/corpus-9f3cae13.tsv): 5324 unique
records from scala/scala revision `3f6bdaeafde17d790023cc3f299b81eaaf876ca3`.
The `9f3cae13` gate compared against `corpus-0828f77b.tsv`: **losses=0,
changes=4**, all fail-to-pass: `run/resetattrs-this`, `run/t3327`, `run/t3984`,
and `run/tuples`.

The `0828f77b` gate compared against `corpus-4cc87fb2.tsv`: **losses=0,
changes=3**, all fail-to-pass: `pos/imports-pos`, `pos/t7233b`, `pos/t8855`.

The `4cc87fb2` gate compared against `corpus-b83e06a8.tsv`: **losses=0,
changes=5**, all fail-to-pass: `pos/Transactions`, `neg/overload-msg`,
`neg/typeerror`, `run/Course-2002-03`, and `run/impconvtimes`.

Historical comparisons before this gate follow.
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
`losses=0` as well; across the accepted main gates no corpus status has gone
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

Use `python3 tests/compare_corpus.py tests/baselines/corpus-5108669a.tsv
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
| `cargo test --workspace --release --no-fail-fast` | **284 result rows, 2655 passed, 0 failed** at `9f3cae13` |
| `tests/spec_classfiles.sh` | `tests=37 match=2 differ=26 no_compile=9`, `$sp` scalac=700 scala-rs=0, **LEDGER RED** |

No compiler source, Cargo input, or test changed after the full run.
`cargo clippy --workspace --release` exits zero with **60** individual warning
messages (excluding per-crate generated-warning summaries). The saved wildcard
baseline log and current log have the same warning-message multiset; none were
added by this slice. Evidence: `/tmp/wildcard-receiver-probe/clippy.log` and
`/tmp/returning-elements-clippy.log`. The old recorded count of 58 was stale.
Compare the same command scope; `--all-targets` also includes test warnings.

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

## The one corpus loss, and how it was closed

Gate eleven was the session's only `losses=1`, and it was honest:
**`neg/name-lookup-stable`**. `agent/nameamb` has since closed it, using
exactly the plan `agent/negchecks` wrote down — `SymbolTable::scopes` already
carries depth as its own index, so the comparison is innermost-`Explicit` /
`Wildcard` scope against innermost-`Definition` scope. The corpus is back to
`losses=0`, `neg` went 673 -> 681, and the fix also closed a *second* wrong
program nobody had reported: `import Priv._` was bringing `Priv`'s own
`private[this]` members into scope and binding them ahead of the importing
class's own definitions, which SLS 5.2 forbids and which compiled and printed.
The history below is kept because the reasoning is what matters.

`agent/libnotype` removed a set of bogus errors — a case-insensitive directory
classpath fabricating `package scala.Math`, a blank line not ending an
expression, and `new X` resolved in the term namespace. Four `neg` tests had
been "passing" *because* of those errors, on messages that appear nowhere in
their `.check` files. The coordinator verified each against its `.check` and
against scalac. `agent/negchecks` then implemented three of the four missing
rules, and those three are byte-identical to their `.check` again. The fourth
is this one: scalac reports `reference to PrimaryKey is ambiguous; it is both
defined in class A and imported subsequently by import ColumnOption._`, and we
**emit nothing at all** — verified directly.

So the pass was never real, and the loss is the compiler telling the truth
about a check it does not implement. `agent/negchecks` reduced it to sixteen
lines and wrote down what closing it needs (`SymbolTable::scopes` already
carries depth; the comparison is innermost-`Explicit`/`Wildcard` scope against
innermost-`Definition` scope, with `BindRank::PackageElsewhere` as nsc's
level-4 exception, and the message needs the definition's owner and the import
clause's source text). It is in `docs/not-implemented.md`.

## What this gate cost that no error count shows

Three of its slices found the compiler getting a *right-looking* answer wrong,
and one of them was hiding under every green check this project has:

* **`agent/sysout`** — `gen_apply` dispatched the `println`/`print` intrinsic
  **by name**, discarding the qualifier. So `java.lang.System.err.println(x)`
  went to **stdout**, in both modes, in ordinary programs. Worse, slick's
  `TreePrinter`'s two-argument `print(n, out)` was hijacked and its
  `PrintWriter` was **never constructed** — a real miscompilation, while
  `slick_measure` said `errors=0 classes=1490`, `verify_all` said
  `verify_failures=0`, `classfile_lint` said `lint_problems=0`, and
  `slick_run` said `progs=12 ok=12 diff=0`. A twelve-program execution check
  proves nothing about the code those twelve programs do not reach.
* **`agent/pickleparams`** — we pickled `def f(): T` as `def f: T`, so real
  scalac reading our class files rejected every call written with parentheses:
  14 errors on the interoperability fixture, now 0. Fixing it exposed a
  *phantom member* — a zero-field case class's `copy()` was pickled and never
  emitted, unreachable until the pickle became right — and an over-correction
  that only `-deprecation -Xfatal-warnings` could see, which the test now runs
  under permanently.
* **`agent/negchecks`** — `localobj.rs` already implemented "nested object is
  not allowed in value class", behind the driver's `if !has_errors(&diags)`.
  A second check firing made the first one's message disappear. **A rejection
  rule cannot live behind a `!has_errors` guard.**

`agent/accessmsg` then rebuilt the access diagnostic the way nsc's
`AccessError` builds it — `underlyingSymbol(sym).fullLocationString` and
`directObjectString`, so `method x in class C … from object C` rather than
`value x … from C$`. It moves no count; it moves three `neg` tests to
byte-identical with their `.check` and the wording-score of the 26 access
tests from 0 to 3.

## Gate twelve: six slices, and four of them corrected the brief

The scala library went **917 -> 852** and cats **185 -> 182**, but the
instructive part is that four of the six found the coordinator's diagnosis
wrong and said so with a measurement.

* **`agent/unitpop`** and **`agent/secondaryctor`** independently reached the
  same conclusion about a `VerifyError: Bad type on operand stack`: the
  `invokespecial` descriptor was **right**, and the argument arrived already
  adapted to the *primary* constructor's parameter type. `erasure::method_param_types`
  answered "what does this `new` adapt its arguments to?" with the class's
  first `<init>` member. One of the two recorded it as unfixed; the other
  closed it and marked the note as agreed. `run/kmpSliceSearch` is the gain,
  and it is a jar class — `new scala.util.Random(Integer.parseInt(…))`, whose
  primary takes a reference and whose picked secondary takes an `Int`, so we
  had been boxing an `int` into an `(I)V` slot.
* **`agent/unitpop`** also fixed the mirror image in the private runtime: a
  discarded `identity(())` left `getstatic BoxedUnit.UNIT` with nothing after
  it, which merely leaks a slot in straight-line code and is
  `VerifyError: Inconsistent stackmap frames` inside an `if`. Its fixture puts
  a branch after every statement-position case for exactly that reason.
* **`agent/siblingover`** disproved the coordinator's hypothesis with
  instrumentation — zero empty-`kept` events across the whole library run — and
  split its twenty candidates twelve/eight. The eight are `Nil` the *module*
  owned by package `scala` beside `Nil` the *term* owned by `package$`: one
  entity supplied twice by routes in no `extends` relation. **A repeated type
  in an `<overload …>` list is what duplicate supply looks like.**
* **`agent/arrayelem`** found the coordinator's `losses=1` expectation stale
  and the actual loss its own: `neg/multi-array`. Its root predated the slice —
  the array path typed every argument as `Int` and never counted them, so
  `new Array[Int](10, 10)` was **accepted on the pre-fix binary too**.

Two of the six also recorded a *cost*: `agent/basetypemeet` measured its rule
at about **+10% compile time** on `src/library` and put the quiet-machine and
loaded-machine numbers on the record rather than choosing the flattering one,
after four measured tuning changes took it down from +25%. `agent/siblingover`
measured its own at no detectable cost, alternating both the binary and the
order within each round because the first attempt had charged the fix for load
drift.

**And the check that caught the most this gate was not a measure.**
`agent/siblingover`'s rule deleted a genuine overload — `same_signature` lets
an abstract `T` match `Int` — and its own words are the lesson: *the library
measure did not catch this; a fixture did.*

## Gates thirteen and fourteen: the gate's own two defects

Both were found by slices, not by the coordinator, and both are the same
failure this script exists to prevent — **a check reporting green over
nothing**.

1. **The corpus ledger was chosen by `ls -t`.** In a git worktree every
   baseline carries the checkout time, so `agent/anyconstr`'s gate compared
   against a 106 KB ledger from eleven gates ago and still printed
   `VERDICT=PASS`. The ledger now comes from the name recorded in this file,
   an unresolvable one is a `FAIL`, and the `gate:` line prints which was used.
2. **Only slick had its measure checked.** `agent/ctorgaps` and
   `agent/triemapjava` reported, independently, a gate that printed
   `VERDICT=PASS` while cats said `measurement invalid: no source files` and
   gitbucket said `files=25` of 353 — both because their **shared checkouts had
   been gutted**, `.git` reduced to an empty skeleton by something racing in
   the scratchpad. Worse, the skeleton directory made each script's own
   re-clone guard (`[[ ! -d $SRCROOT/.git ]]`) never fire, so every later run
   would have gone on reporting `files=25` in silence. All four measures now
   have their `files=` count pinned and `measurement invalid` is a `FAIL`,
   exercised on all three shapes of breakage before landing. The checkouts
   were repaired — gitbucket in place by `agent/triemapjava`, cats by deleting
   the skeleton so its script re-cloned.

The library went **852 -> 740** across these two gates. The largest single
contribution is `agent/triemapjava`, which took the worst file in the measure
(`TrieMap.scala`, 46 errors) to 10 — and whose root turned out not to be
Java-specific at all: `type_args_are_instantiated` refused any type argument
that *is* one of the instantiated class's own type parameters, which is
exactly what `new C[K, V]` writes from inside `C` and is indistinguishable,
*from the type alone*, from an un-applied `new C`'s placeholders. It also
found that reading a Java instance field emitted an illegal method name —
`ClassFormatError` on any class reading any Java instance field, invisible to
every compile-time measure — and that field `Signature` attributes were read
and discarded, so a raw type conformed to every instantiation.

**gitbucket went 270 -> 271, and that is stated rather than netted.**
`agent/triemapjava` diffed against the saved log: exactly one new site,
`EditorConfigUtil.scala:129`, nothing fixed. Reading the `Signature` correctly
makes `PropertyType.tab_width` a `PropertyType[Integer]`, which pins
`T := Integer` against a Scala `Int` argument — and overload *scoring* does
not consider the boxing that adaptation would apply. Either candidate alone
compiles, so the gap is pre-existing and separate; the pre-fix binary accepted
the call only because it did not know what `tab_width` was.

## Gates fifteen and sixteen: two wrong answers behind one message

Both slices were briefed as message families and both turned out to be
**accepted programs that could not run**. Neither was visible in any error
count, because the count only sees what we reject.

1. **A repeated parameter's `Seq` was a name looked up in scope.** nsc's is
   `definitions.SeqClass`, a fixed symbol; ours was whatever the scope bound.
   That was wrong in both directions. It found a *user's* `Seq` --
   `object Main { class Seq[A] { def tag = "MINE" }; def f(xs: Int*) = xs.tag }`
   compiled, and since `gen_desc` writes `Lscala/collection/immutable/Seq;`
   for a repeated parameter whatever the typer concluded, the emitted
   `invokevirtual Main$Seq.tag` met an `ArraySeq$ofInt` and threw
   `ClassCastException` at run time with no diagnostic anywhere. And it found
   *no* `Seq` when the standard library is compiled from source, where the
   only binding is `scala/package.scala`'s alias, so every repeated parameter
   in the library was left as the bare `T*`. `agent/varargsrecv` also
   implemented nsc's `*-parameter must come last`, per parameter clause, which
   we had silently accepted; `neg/parstar` is scala/scala's own test for it.
2. **A Java varargs call with a primitive element type could not run.**
   `gen_java_varargs_array` boxed into `anewarray java/lang/Object`, so
   `f(int...)` was `VerifyError: '[Ljava/lang/Object;' is not assignable to
   '[I'`. Only reference element types worked, and nothing noticed because the
   library never calls the varargs side. Found by `agent/javavarargs` while
   fixing the resolution defect above it -- and its sibling, a `f(xs: _*)`
   splice accepted by a *fixed-arity* alternative, only became reachable once
   that alternative could win.

The resolution rule itself is worth recording, because "prefer the fixed-arity
alternative" is wrong. `agent/javavarargs` measured six pairs against scalac
2.13.16, three of them read back from javac's class files: `f(Object)` /
`f(Int*)` on `f(1)` and `f(Int, Int*)` / `f(Int*)` on `f(1)` are **ambiguous**,
and we had been *accepting* the first. The rule is nsc's `Infer.isAsSpecific`
asymmetry -- a repeated parameter is unwrapped to its element type only when
both signatures are varargs lists -- and it is not Java-specific at all;
`mutable.Buffer`'s own `prepend(A)` / `prepend(A*)` had the same defect.

**The composition is worth more than either slice.** Separately the library
measured 685 and 727; together it is **672**, because `javavarargs`' first
attempt closed only 2 of its 12 sites -- `seq_of`'s scope dependence, the very
thing `varargsrecv` fixed, was what stopped the rest. Two slices with different
briefs found the same seam from opposite sides.

Both slices also corrected their briefs. I handed `varargsrecv` `Array.scala`
and `collection/Seq.scala` as one family; `Seq.scala` contains **no** repeated
parameter at all -- its 21 errors are `val (elms, idxs) = init()`, a tuple
pattern definition whose component types are never instantiated, now the
largest single mechanism in the remaining 672. I handed `javavarargs` twelve
Java sites; the thirteenth was Scala and in the standard library.

## Gates seventeen and eighteen: two briefs the slices had to throw away

`agent/tuplepat` was briefed on tuple pattern definitions, which the previous
slice's measurement had identified as the largest remaining mechanism in the
library (21 of `collection/Seq.scala`'s 31 errors). **Pattern definitions were
never broken.** They compile, they evaluate the right-hand side once, they
throw `MatchError` on a refutable pattern, and the slice's fixture executes six
shapes of them against scalac's own output to say so. The `T1`/`T2` in the
errors were the *residue of a failed right-hand side*: when one component of a
tuple expression fails to type, `Tuple2`'s parameters stay uninstantiated and
the pattern definition faithfully hands that to every name it binds. **The
messenger looked like the culprit because it is the thing that repeats.**

Three unrelated roots were behind it, all confirmed against scalac 2.13.16:

1. **`private[this]` through a trait's self-alias.** nsc's `isAccessible` asks
   the prefix *type* (`pre =:= sym.owner.thisType`); we asked whether the tree
   was a `This` node. `trait SeqOps { self => … }` writes `self.toGenericSeq`
   as an `Ident`, so the syntactic test could never pass, and no
   `private[this]` member of the library was reachable through its own alias.
2. **An inherited factory `apply` called as `Obj[K, V](…)`**, taken at its
   declaration instead of as seen from the module, so
   `mutable.HashMap[A, Int]()` returned `CC[A, Int]`. 30 errors.
3. **`xs.to(Factory)` with an abstract element type**: the guard meant to
   check that no *unknowns* remain rejected any type parameter at all,
   including an enclosing class's own fixed one. `List[Int]` worked and
   `List[A]` did not, which is why the shape survived this long.

Fixing (2) made a call *reachable* — and it was mis-compiled. The redirect left
the receiver a bare `Ident`, which in a class body means `this`, so
`Fac.apply` was emitted against `this`: `ClassCastException` at run time, with
the verifier, the class loader and the lint all silent because the types agree.
**Gate seventeen's corpus is `changes=0`.** It closed 50 library errors, 5 cats
errors and one silent mis-compile, and moved no test status at all.

`agent/ctorgaps2` **corrected a note this file's predecessor documents.** The
recorded warning was that `private[p]` constructors must not be closed by
tightening the flag test, because that would reject every `private[slick]`
constructor slick itself calls. There is no flag to tighten: scalac 2.13.16
pickles `class Qual private[libp] (…)` with `flags=0x200`, `PRIVATE` and
`PROTECTED` both clear, and the boundary in `privateWithin`. Reading it
properly then exposed two holes — a constructor installed but not repaired
carried no `CONSTRUCTOR` flag and **skipped the access check entirely**, and
the descriptorless partial symbol was counted as another callable constructor.
The writing half stays open (emitting `privateWithin` moves every `SymInfo`
entry after it) and is pinned by a test.

Its second gap is the session's recurring lesson in miniature: implementing
nsc's "prefer the alternative that needs no default" **turns a refusal into a
silent wrong answer** on its own. `new Three(2)("m")()` folds to `(2, "m")`,
which a primary `(Int, String)` accepts exactly, and prints `m` where scalac
prints `m/m2`. nsc selects on the first written clause; holding the pick to the
same alternative set the fold measured its arity against is what makes the rule
safe.

## Gate nineteen: the ledger is unchanged and that is the point

`agent/hkfield` moved no compile measure and no corpus status. It could not:
the change is in the backend, and every measure that counts errors stops before
code is emitted. What it closed is a `VerifyError` -- a member declared at a
**bounded** type parameter erases to its bound, not to `Object`, and both
erased-load sites tested for the literal `Ljava/lang/Object;`. The class files
were emitted with no diagnostic and the JVM threw the method out at run time.

Briefed as higher-kinded, it is not: the slice found the first-order
`class BHolder[T <: Boxy[Int]](val f: T)` broken identically, and higher-kinded
shapes only looked special because an unbounded parameter erases to `Object`.
The decidable question was already written one function away, on the method
*result* path (`is the declared erasure known to conform to what we want?`),
which is why `def m: F[A]` was correct while the field beside it was not.

The second site was found by attacking neighbouring shapes after the first went
green: a case class's synthetic `unapply` has no body, so the match reads the
constructor field directly, through its own copy of the same `Object`-only
test. The fixture's pattern case had been passing through the `Select` path,
so fixing one site left the other standing and silent.

Its test reads `javap -c` and asserts not only that a `checkcast` appears where
scalac puts one, but that **none appears** where scalac emits none -- an
unnecessary cast is a divergence too, and running green does not show it.

## Gate twenty: the inference was right, the supply was not

`agent/lowerbound` was briefed on `Infer.methTypeArgs` solving a lower-bounded
`B >: A` to its bound before looking at the argument. It measured that instead:
a `def sortedX[B >: A](implicit ord: Ordering[B]): C` **defined in source**
already matched scalac at the branch point, both with an explicit argument
(`B := AA`) and through implicit search (`B := A`). What diverged was the
*supply* of signatures.

* `pin_undetermined_tparams` dropped every type parameter no explicit
  parameter names, pinning it to its lower bound. **Its own doc comment gave
  `def max[B >: A](implicit ord: Ordering[B]): A` as the example and asserted
  that scalac solves it that way.** It does not — an argument passed to the
  implicit clause decides `B`.
* `List`'s `sorted` / `min` / `max` / `sum` / `product` are hand-written in the
  prelude with no type parameter at all, while every other collection arrives
  through the pickle. That is why `List` alone diverged.

The pin is kept for the one shape that genuinely needs it — a bound parameter
that no parameter mentions, as in cats' `Resource#allocated[B >: A]` — because
removing it outright cost slick five errors.

**The gate rejected two of the three attempts, each for a different reason,
and that is the record worth keeping.** Preferring the expected type over the
lower bound lost `run/t10513` (`Numeric[Any]`): nsc's `solvedTypes` minimises
covariant variables and an expected type is only an upper constraint. Using
the declared `bound_lo` unchanged broke a self-type receiver with no receiver
tree to read the class's type arguments from. The slice's first gate printed
`VERDICT=FAIL` with four workspace failures and `losses=1`.

Its negative fixtures are the more interesting half. `List[String].sum` was
**accepted** at the branch point, with no diagnostic, whichever way the
parameter was handled; and the rejection message for a bound violation matches
nsc down to naming the join it settles on (`required: Ordering[Any]`) rather
than the declared bound.

### Left open by this slice, measured and named

`List`'s prelude has four more holes of the same shape, found with a probe that
enumerated every `[B >: A]` member against a `Sup`/`Sub` pair: `indexOf[B >: A]`
and `contains[A1 >: A]`, `reduce` / `reduceLeft[B >: A]` / `reduceRight[B >: A]`,
and `Map.+[V1 >: V](kv: (K, V1))` (one cats error,
`WrappedMutableMapBase.scala:28`). All close through the same
`prelude_lowbound.rs` mechanism; they were left because `sum` changing from `A`
to `B` needs its erasure and unboxing re-checked. `startsWith` was fixed
incidentally by this slice.

Two things the brief grouped with this root are **not** it: the `Equiv` /
`Ordering` / `Hashing` mismatches in cats are lambda-to-SAM conversions, and
`NonEmptyVector.scala:287`'s `found: Seq[A] required: Vector[A]` is `sortBy`'s
return type — the `C` of `IterableOps[A, CC, C]`, which is `agent/basetypeargs`'
subject. `sortBy[B](f: A => B)(implicit Ordering[B])` has no lower bound at all.

## Gate twenty-one: a symbol that existed twice

`agent/basetypeargs` was sent after `SymbolTable::base_type_args`' first-path
behaviour, which two earlier slices had named 「本命の修理」. **It was already
closed**, by `agent/basetypemeet` two gates before, and this file's own record
said so — the coordinator handed on a documented next step without checking
whether it was still open. Nothing in this slice is in the base-type walk.

What it found instead is worth more than the eight errors it moved.
`Iterator.sliding` returns the nested `Iterator.GroupedIterator`, and
`ensure_class` split `scala/collection/Iterator$GroupedIterator` on the last
`/` only — producing a **second symbol** whose simple name was
`Iterator$GroupedIterator` and whose owner was the package, alongside the
correct `Iterator.GroupedIterator`. Which one a program got depended on **reach
order**: `def x[A](it: Iterator[A]): Iterator[Seq[A]] = it.sliding(2)` was
refused, and putting `it.sliding(2).next()` one line above made the same
expression compile. A test pins both orders now.

**The gate refused two broader versions of the rule before this one.** Applying
the repair to every nested library class cost slick two errors, ten workspace
tests and `losses=3`, because `scala.reflect`'s API is hand-built by
`prelude_reflect` rather than read from the pickle and the macro code depends
on those symbols; narrowing it to every nested `scala.collection` class then
broke `MapOps.WithFilter`, which is not an `IterableOnce`. A third version that
attached parents and rolled them back left `self.parented` marked and killed
the lazy path. The rule that survives is name-only and decided before any
symbol is made. All three discarded versions are recorded in `docs/cats.md`.

Two things the slice measured and did **not** fix, both named precisely:
cats' four remaining `grouped` errors are a different root that **does not
reproduce outside the full run** — compiling `NonEmptyVector.scala` alone with
the same flags and classpath produces the other 130 errors and not these — and
`Iterator.GroupedIterator` is still *accepted* as a spelling scalac rejects,
because the rule that makes it work is the same one that carries
`Resource.ExitCase`.

## Gate twenty-two: the one that accepted too much

`agent/preludelb` was handed the four holes `agent/lowerbound` had left named.
It re-ran the probe as a **two-directional accept/reject comparison** against
scalac 2.13.16 -- 71 calls over `List`, `Map`, `Set` and `Option`, each
compiled by both compilers -- and found **eighteen** divergences. The recorded
list was incomplete in both directions.

The one that matters is `Map.updated`, the only member that **accepted too
much**: it took the widening argument and answered at the un-widened type.
`Map` is covariant in `V`, so `val m: Map[K, Animal] = md.updated(k, cat)`
conforms either way and only a narrow ascription separates the two answers.
The branch point compiles the negative fixture **with no diagnostic at all**.
A list built by looking at error messages could never contain this member,
because it produced none. `List.toArray[B >: A](implicit ClassTag[B])` was the
other omission -- a sixth member of the `sorted` family's exact shape.

The erasure question that stopped the previous slice is settled by measurement
rather than argument: **zero bytecode diff lines** against a branch-point
binary for every call both compilers accept, and boxes and unboxes at the same
points as scalac. At `List[Int].reduce[Any]` neither unboxes; at
`List[Dog].contains(cat)` both `checkcast`.

gitbucket moved 265 -> 264 and gained one new error at the same line, which is
stated rather than netted: `+` now type-checks the pair, so the disagreement
moves out to the result -- our lub over an invariant `Set` gives
`Map[A, Set[_ <: A]]` where nsc lands on `Set[A]`. More accurate, not a
regression. The corpus gain is `pos/t2179`, which only compiles with `[B >: A]`.

Left named and not fixed: `Map`'s key parameters are `Any` rather than `K`, so
`md.apply(1)` on a `Map[String, Dog]` is accepted here and rejected by nsc --
five members at once, deliberate per `prelude_ovl3` and independent of the
bound. And a lower bound naming **another variable of the same call**
(`def put[V, V1 >: V](m: List[V], v: V1)`) is solved from its argument alone
instead of jointly; that one is defined in source, not the prelude, and is the
sharpest remaining item in this area.

## Gate twenty-three: a flag two supply paths cannot set

`agent/samconv` was briefed on missing SAM conversion. **It is not missing** --
it works for source-declared traits and for Java interfaces, and has since
`sam_runnable_and_comparator_typecheck`. What did not work was a SAM type the
compiler *read*. `SymbolTable::sam_sig` counts abstract methods by
`Flags::ABSTRACT`, and **neither path that builds a library class can set that
flag**: `prelude::method` forces `FINAL` on every member and `PickleSupply`
reserves with `EMPTY`. Both are deliberate, for reasons already written in
`override_check::modifiers_are_known`. So `scala.math.Equiv` was read as having
zero abstract methods and was not a SAM type at all -- and neither was anything
else that arrives through the pickle.

`Ordering` needed a second answer underneath: `PickleSupply` enters members by
name **on demand**, so an override nobody has asked for cannot be told apart
from an absent one, and `Ordering`'s concrete `equiv` (inherited deferred from
`Equiv` through `PartialOrdering`) was missing until something requested it.
The fix reads the pickle for the answer rather than entering the members --
**decided by measurement**: the version that actually completed them regressed
cats' `NonEmptyVector.scala` from 4 diagnostics to 5, because completion is
additive global state, a hazard its own doc comment already records.

Two pre-existing defects surfaced with it, both reproducible at the branch
point through a source-defined trait: an arity guard that **could never fail**
(`param_tys.len() == pts.len()`, forty lines after `pts` has already been
collapsed), so `val e: Equiv[Int] = (x: Int) => x > 0` compiled; and a
polymorphic abstract method treated as a SAM, which nsc's `samOf` refuses by
requiring `sam.typeParams.isEmpty`.

**Two of this brief's claims were the coordinator's, and both were wrong.** It
asserted `crates/cli/tests/samfwd.rs` exists at the branch point; it does not,
and the concern behind `agent/samfwd` (a trait's concrete methods living in a
`T$class` static, needing forwarders in the anonymous class) has since
evaporated -- this backend emits JVM `default` methods now. The lesson is the
same as gate twenty-one's: a documented next step has to be re-checked against
the tree before it is handed to a slice.

Left named: our SAM literals are always anonymous classes, where scalac 2.13.16
uses `invokedynamic` for `Equiv`, `Hashing` and `Runnable` in the same file and
an anonymous class only for `Ordering`. Behaviour is identical (the fixture
executes against scalac's own output); the divergence belongs to
`crates/cli/tests/indy.rs`.

## Gate twenty-four: `implicit class` was invisible in every library

`agent/gbslickmember` was sent after 35 gitbucket errors shaped like
`value withTransaction is not a member of DatabaseDef`, on the observation
that we compile slick itself to 1490 byte-exact class files, so the members
exist in what we produced and something loses them on the way back.

The root is one line of filtering. nsc expands `implicit class C(x: T)` into a
plain `class C` **plus** an `implicit def C(x: T): C`, and marks that
conversion method `SYNTHETIC` — pickled flags `0x200201` =
`IMPLICIT|METHOD|SYNTHETIC`. `Member::is_public_api` filtered SYNTHETIC out.
**The class file cannot stand in for it, because nothing in bytecode records
`implicit`**, so what the class-file reader installs is an ordinary method:
in scope under its own name, callable explicitly, and never selectable as a
view. `BlockingDatabase(db).withTransaction {…}` compiled the whole time and
`db.withTransaction {…}` did not. This was true of **every `implicit class` in
every library on `-cp`**, including one declared at top level.

**It corrects the diagnosis this repo carried.** `docs/gitbucket.md` recorded
the cause as the result being an inner class of the imported value's own type,
with an `$outer`. `queryToQueryInvoker`, which always worked, returns an inner
class with an `$outer` too. The separator is `implicit class` against
`implicit def` — one keyword in blocking-slick's source — and nothing in the
pickle reader's projection or prefix handling needed to change.

The gate refused two earlier versions, both real:

1. Admitting the rule for `scala.*` owners took `DurationInt` off the
   hand-written prelude, so `3.seconds` emitted an `invokevirtual` on an `int`
   — a **VerifyError, not a type error** — and took `Quasiquotes.Quasiquote`
   off the quasiquote path. Scoped to non-`scala.*` owners, where the prelude
   is the authority.
2. Adopting a *module* import prefix re-entered its members beside the
   class-file ones: `pos/t5639` became `ambiguous implicit: Baz, Baz, Baz`.

Of the 26 errors that went, all are the one root; of the 4 that appeared, three
are cascades behind an `sql"…"` macro expansion that already fails at the same
line, and one is a pre-existing false diagnostic becoming reachable in a second
file. Stated rather than netted.

Left named and not fixed: `value returning is not a member of TableQuery[…]`
(10) and the 31 `Shape` missing-implicits **did not move by a single line** —
`returning` is not declared by an `implicit class` at all. The brief grouped
four message shapes as possibly one root; they are at least two. And a package
wildcard hides the prefix of a later member import: with `import iclib._`,
a following `import profile.api._` brings in nothing at all, not even a class
under its own name. Six lines reproduce it; gitbucket writes explicit imports,
so no measurement shows it.

## Gates twenty-five and twenty-six: two probes, six accepted programs

Both slices were briefed on a count of error messages and both returned
something the count could not contain.

`agent/strarrayops` was sent after 16 `value apply is not a member of String`
and `stepper` errors, on the theory that `Predef.augmentString` was not
supplying members and that a **source** `Predef` behaved differently from the
prelude's. **Every part of that was wrong.** Writing the conversion by hand
worked; selecting on `StringOps` directly worked; 29 of 32 probed members
worked. The root is that `search_extension` had **no owner tie-break** — nsc's
SLS 6.26.3 rule that a member inherited from a base class loses to one declared
in the derived class. `object Predef extends LowPriorityImplicits` puts
`augmentString` in `Predef` and `wrapString` in the base; both take a bare
`String`, so every tie came out level and the search returned `None`. Thirteen
lines of user code mentioning neither `String` nor `Array` reproduce it in both
modes. Jar mode appeared healthy only because the hand-written prelude carried
the same fact as two `low_priority` booleans — **concealment, not a different
root**.

`agent/varassign` was sent after 8 `reassignment to val initBlank`. Those are
not a mutability defect at all: they are **named arguments in a
self-constructor delegation**, typed positionally because
`type_ctor_delegation` had no named-argument handling, so `initBlank = true`
became an `Assign` against a primary-constructor parameter that genuinely is in
scope there. `new C(b = 2, a = 1)` and `extends B(b = 2, a = 1)` were fixed by
earlier slices; `this(...)` was the third path and nobody had looked at it.

**Its two-directional probe is the part worth keeping.** 47 snippets, each
compiled here and by scalac 2.13.16 and compared on accept/reject: 19
disagreements, and six were programs this compiler **accepted and carried to
the backend**. `def v: Int = 1; v = 2` emitted a `putfield` to a field the
class does not have. `object O; O = null` likewise. A `val` inherited from a
class file was assigned through a `putfield` to someone else's private field.
And the sixth — the one **scalac also accepts**, so the one with no message on
either side — assigned a `var` inherited from a class file through a `putfield`
to a private field: `IllegalAccessError` at run time, verified by building the
superclass with real scalac and running it. The probe is committed as
`tests/assign_probe.sh`.

Both slices left a live silent mis-compile named rather than half-fixed:
assignment to a **wildcard-imported** `var` uses `this` as the receiver
(`import O._; ov = 5` → `ClassCastException`), while the *read* of the same
name is correct. `import_named` calls `remember_named_import_prefix`;
`import_wildcard` is not even given the prefix tree. `agent/unqualname` reached
the same mechanism from the other side (calling an *inherited* member through
`import <object>._`), so it is one root with two symptoms.

## Gate twenty-seven: unqualified names on the composed tree

`tests/verify_merge.sh` ran once without skipped stages on clean `4cc87fb2`,
which merges `03ec631e` and `1583aa49`. It completed with `VERDICT=PASS` and
`DONE`. Logs: `/tmp/scala-rs-gate-4cc87fb2-codex/gate.log`.

The library measure improved from 560 errors in 124 files to **541 in 123**;
gitbucket remained 242/72 and cats 163/62. Slick remained 184 source files,
zero errors, and 1490 classes, with `verified=1490 failed=0` and
`lint_problems=0`. MODE=b executed 12/12 programs with 36/36 attempts.
Workspace tests passed 2649/2649; the full corpus gained five statuses with
zero losses, including two negative tests previously accepted. The gate's
format check passed. Main was fast-forwarded to the exact tested commit;
the following recording commit changes only this file and the saved corpus
ledger, not compiler code or test inputs.

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

## Rejected intermediate wildcard receiver candidate: `7220a0ec`

Gate `/tmp/scala-rs-gate-7220a0ec-codex/gate.log` completed without skipped
stages and printed `VERDICT=PASS`, but that tree was **not approved for
merge**: gitbucket rose from 242 to 290 errors (72 files). The script checks
input completeness, not gitbucket error-count regression. An eight-line
case-class companion with a named custom `apply` compiles on main and scalac
but fails on this candidate: qualifying the imported module loses the custom
`apply` selection. This is a real regression despite the script verdict.

Cats stayed 163/62, the library 541/123, slick 0 errors / 1490 classes with
12/12 execution, zero validation failures and lint problems. Workspace:
2651 passed, zero failed. Corpus: pos 1109, neg 690, run 631; losses=0,
changes=3 (`pos/imports-pos`, `pos/t7233b`, `pos/t8855`). The candidate ledger
is `tests/baselines/corpus-7220a0ec.tsv`; it did not replace the accepted baseline.

## Gate twenty-eight: wildcard receivers belong to an import binding

The final `0828f77b` gate completed with `VERDICT=PASS`, `DONE`, no skipped
stages and corpus losses=0 against `4cc87fb2`. Logs:
`/tmp/scala-rs-gate-0828f77b-codex/gate.log`. Main was fast-forwarded to this
exact commit; the subsequent record changes only this file and the ledger.
Gitbucket improved 242/72 -> 239/71, cats remained 163/62 and the library
541/123. Slick retained zero errors, 1490 validated classes, no lint problems
and 12/12 MODE=b programs (36/36 attempts). Workspace: 2651 passed, zero
failed. The corpus gained the three positive statuses listed above.

The initial diagnosis was incomplete. Direct object-field reads worked, but
an *inherited* var read also threw `ClassCastException`, alongside direct var
assignment and inherited method invocation. A binary built from `a63c4d04`
compiles the new fixture and fails at `WildMain.inheritedRead`; real scalac
2.13.16 executes it. The fixture now compares stdout bytes in both ABI modes
with scalac, including reads, writes, inherited calls, aliases, nested imports
and an overloaded case-class companion. Six accept/reject probes compare both
directions, and a negative fixture pins assignment to an imported val.

The first fix used the class-to-prefix cache. Nested imports through two
objects inheriting the same declaration then returned 8 where scalac returned
14: a class is not an import binding. The final path uses the existing binding
origin and lexical scopes. Alias selection uses the member's original name.
A private var which the eager walk excluded was reintroduced by lazy wildcard
lookup; the latter now applies the same privacy rule. Companion module
references keep their identity rather than being rewritten as ordinary value
selections; the intermediate gitbucket regression above motivated that test.

`cargo clippy --workspace --release` exited zero; existing warnings remain,
with none reported in the changed receiver-resolution code. The gate's format
check passed. This does not claim that all lazy classpath import paths or
legacy value-prefix handling are complete; those remain separate probe targets.

## Rejected returning-family candidate: `78cc806f`

The full gate completed with `VERDICT=FAIL` and `DONE` on clean `78cc806f`.
Logs: `/tmp/scala-rs-gate-78cc806f-codex/gate.log`. This candidate is not
accepted and does not replace the main baseline above.

Gitbucket: 230 errors / 69 files (accepted baseline 239/71); cats: 163/62;
library: 541/123. Slick: zero errors / 1490 classes, verified 1490,
failed 0, lint problems 0, execution 12/12 programs and 36/36 attempts.
Workspace: 2648 passed, 4 failed (`fixtures_am_pickledup`,
`bf_coll_bad_is_still_rejected`, `bf_map_map_without_a_pair_is_an_iterable`,
`bf_coll_runs_against_the_jar`). Corpus: pos 1107, neg 690, run 632;
losses=2, changes=3 against `0828f77b`. The losses are `pos/spec-asseenfrom`
and `pos/t3774`; the gain is `run/t3984`. The saved candidate ledger is
`tests/baselines/corpus-78cc806f.tsv`. Format checking passed.

The error reduction is not sufficient evidence of correctness: non-pair
`Map.map` now throws `ClassCastException`, and `Vector.map` becomes ambiguous
after earlier member lookups. The corrected pickle linearization exposes
existing overload collapsing that discarded `IterableOps` alternatives.
The ten `returning` errors disappear, but three new gitbucket diagnostics
appear around a non-pair `Map.collect`. No implementation from this candidate
has been merged into main.

## Rejected returning-overload candidate: `ebaa481e`

The full gate completed with `VERDICT=FAIL`, `DONE`, and no skipped stages
on clean `ebaa481e`. Logs: `/tmp/scala-rs-gate-ebaa481e-codex/gate.log`.
This does not replace the accepted main baseline.

Gitbucket improved 239/71 -> 228/69; all ten returning diagnostics disappeared
and the error-message multiset gained no entries. Cats stayed 163/62, library
541/123. Slick retained zero errors / 1490 classes, verified 1490, failed 0,
lint problems 0, execution 12/12 programs and 36/36 attempts. Workspace:
2653 passed / 0 failed. Format checking passed.

Corpus: pos 1109, neg 690, run 634; losses=1, changes=5 against `0828f77b`.
`run/t3603` regressed. Gains: `run/resetattrs-this`, `run/t3327`, `run/t3984`,
`run/tuples`. The previous two positive regressions are recovered. The ledger
is `tests/baselines/corpus-ebaa481e.tsv`; no code from this gate is merged.

The new loss executes `IntMap.map` and `LongMap.map`. Their own declarations
are dropped because several JVM methods have identical erased parameter lists
and distinct return descriptors. The generic StrictOptimizedMapOps method then
builds an ordinary Map, followed by a cast to IntMap: `ClassCastException`.
Evidence: `/tmp/returning-t3603/debug.log`, `run.log`, and disassembly of Test$.
The next correction must resolve the JVM declaration's return descriptor too;
merely preferring a receiver's method would reintroduce the Map concatenation
miscompile. Key-preserving, key-changing and non-pair transformations need
execution comparisons with real scalac before another full gate.

## Rejected map-result candidate: `edbdfead`

The full gate completed with `VERDICT=PASS` and `DONE` on clean `edbdfead`,
without skipped stages, but the candidate is rejected for measured library
compilation regressions. Logs: `/tmp/scala-rs-gate-edbdfead-codex/gate.log`.
The accepted baseline remains `0828f77b`.

Gitbucket: 273 errors / 77 files (accepted main 239/71; previous candidate
228/69). Cats: 165/62 (main 163/62). Library: 541/123. Slick: zero errors,
1490 classes, verified 1490, failed 0, lint problems 0, execution 12/12 with
36/36 attempts. Workspace: 2654 passed / 0 failed. Format check passed.
Corpus: pos 1109, neg 690, run 635; losses=0, changes=4. Gains:
`run/resetattrs-this`, `run/t3327`, `run/t3984`, `run/tuples`. The `t3603`
regression is recovered. Ledger: `tests/baselines/corpus-edbdfead.tsv`.

Skipping collection-result rebuilding for all pickled map declarations caused
Seq, Queue, LazyList and Map results to remain wider than the receiver's
collection constructor. The script does not enforce these error counts, so
its PASS does not authorize merging this known regression. A minimal generic
Queue method is rejected by edbdfead and accepted by real scalac.

## Gate twenty-nine: returning families and complete overload identities

Clean `9f3cae13` completed the full gate with `VERDICT=PASS`, `DONE`, and
corpus losses=0 against `0828f77b`. Logs:
`/tmp/scala-rs-gate-9f3cae13-codex/gate.log`. Main was fast-forwarded to the
exact tested commit; the following record changes documentation and the saved
corpus ledger only, with no compiler, Cargo or test-input changes.

Gitbucket improved 239/71 -> 228/69. All ten returning diagnostics disappear;
no error-message entries are added. Cats stays 163/62 with no newly failing
source lines: four lazyZip ambiguity diagnostics advance to the following
BuildFrom requirement at the same call sites. Library stays 541/123. Slick
keeps 1490 verified classes, no lint problems and 12/12 execution (36/36
attempts). Workspace: 2655 passed / 0 failed. Corpus gains four run statuses
listed above. The rejected intermediate candidates remain recorded separately.

The brief's implicit-class hypothesis did not explain returning. The actual
conversion is queryInsertActionExtensionMethods. Source import-prefix expansion,
loading enclosing concrete type aliases, and pickle diamond linearization all
needed correction. The probes compare source and scalac-produced binary
libraries in both ABI modes. Correct linearization exposed discarded collection
overloads: their parameter structures, original declaring owners, companion
identity and JVM result descriptors must survive member supply together.
IntMap/LongMap then exposed a silent bad result-type rebuild. The final rebuild
preserves value parameters and two-parameter Map results, while retaining element
constructor rebuilding for Seq, Queue and LazyList. Runtime fixtures compare
stdout bytes with scalac and both compilers check their negative assignments.

This closes returning's measured ten-error family, not compilation of gitbucket
or cats as a whole. Shape-related diagnostics remain unchanged (12 matching
Shape[ in these logs); MODE=a and specialization obligations remain open.


## Rejected value-class bridge candidate: `70a2eaff`

Clean `70a2eaff`, containing current main `db059c9b`, completed the full gate
with `VERDICT=FAIL`, `DONE`, and no skipped stages. Logs:
`/tmp/scala-rs-gate-70a2eaff-codex/gate.log`. No implementation from this
candidate is accepted. The accepted baseline remains `9f3cae13`.

Gitbucket 228/69, cats 163/62, and library 541/123 are unchanged, including
the error-message multisets. Slick retains zero errors / 1490 classes,
1490 verified, zero validation failures or lint problems, and 12/12 execution
with 36/36 attempts. Format checking passed.

Workspace: 2658 passed / 3 failed (285 rows). Failures:
`arrow_resolves_when_the_source_supplies_predef`,
`recompilation_preserves_main_forwarder`, and
`separate_compilation_package_object_value_class_and_operator_name`.
Corpus: pos 1109, neg 692, run 633; losses=3, changes=6 against `9f3cae13`.
Losses: `run/t10646`, `run/t13022`, `run/t6385`. Gains:
`neg/t6260-named`, `neg/t6260c`, `run/t6260b`.
Saved ledger: `tests/baselines/corpus-70a2eaff.tsv`.

The six focused value-class tests pass, including both binary compilation
directions and anonymous-vs-named bridge collisions. However, substituting
generic underlying representations exposes missing receiver adaptation at
value-class extension calls: the incremental-forwarder test already fails in
its first compilation at `3.moo`, with an int passed to an Object slot.
The remaining failures must be investigated before another gate. Clippy exits
zero; its warning-message multiset has no additions versus the saved previous
log (60 -> 59). This recording commit changes only this file and the candidate
ledger; main's compiler remains unchanged.
