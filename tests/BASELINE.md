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

| commit | `b969b0d1` |
|---|---|
| updated | 2026-09-11 |

**Forty-eight composed gates have been accepted this session**, covering the earlier
ninety-nine slices, the type-identity/macro-transport batch, and the combined
SQL, constructor-storage, value-class-access and reflection-parent batch,
the Forms inference and Scala/JVM name interoperability batch, and the
collection result, evidence factory and Java member batch, and the dependent
result, SAM, implicit override and self-type batch.
Twenty-three intermediate candidates were rejected, three despite a PASS script verdict. From
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
| `598ceef1` | `value-class bridges` | 228 | 163 |
| `218b5340` | `lazyZip declaration origins` | 228 | 163 -> **159** |
| `88ab9308` | `binary parent prefixes and lexical companions` | 228 | 159 |
| `172a6525` | `conversion witnesses and singleton inference` | 228 -> **223** | 159 |
| `5adf9c87` | `inherited results and constructor evidence` | 223 -> **217** | 159 -> **158** |
| `caf8f284` | `early lexical scopes` | 217 -> **202** | 158 |
| `02317f15` | `inherited factory redirect` | 202 -> **192** | 158 |
| `176629aa` | `partial factory inference` | 192 -> **191** | 158 -> **152** |
| `a7f05f48` | `App and DelayedInit identities` | 191 | 152 |
| `e355ab70` | `directory Scala signatures` | 191 | 152 |
| `5f0d18c2` | `immutable Map key types` | 191 | 152 |
| `647afcf2` | `Java completion`, `Unit function arity`, `Either companion` | 191 -> **188** | 152 -> **139** |
| `3343f368` | seven member/application mechanisms | 188 -> **169** | 139 -> **125** |
| `23031519` | five evidence/default-import mechanisms with namespace integration | 169 -> **158** | 125 -> **121** |
| `73f68974` | type identity, binary implicit objects, macro transport and source ownership | 158 -> **157** | 121 |
| `603b6451` | SQL macro argument types, Java constructors, storage/access/defaults and reflection parents | 157 -> **148** | 121 |
| `9cc076f6` | nested Forms inference, encoded names, literal types and nested class ownership | 148 -> **115** | 121 |
| `913fb1c0` | collection results, evidence factories, Java members and full gitbucket inputs | **108** (115 on historical inputs) | 121 -> **103** |
| `b969b0d1` | dependent results, SAM inference, implicit overrides and self types | 108 -> **105** | 103 -> **90** |

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
| `tests/slick_measure.sh` (184 files) | **0** | **0** | **1504** |
| `tests/cats_measure.sh` (339, 1 skipped) | **90** | **37** | — |
| `tests/gitbucket_measure.sh` (354, none skipped, 3 real Java sources) | **105** | **49** | — |
| `tests/scalalib_measure.sh` (538) | **421** | **114** | — |

Gate `913fb1c0` expanded gitbucket input coverage: that candidate on the previous
353-source, Java-disabled input reported **115/54**. Its expanded **108/52** result
includes the previously excluded controller and all three actual Java helpers.
This difference is not reported as a compiler-only improvement.

The file counts above are the scripts' reported metric (`grep -A 2` after
error headers). Multiline diagnostics can escape that two-line window. The
complete current diagnostic parser finds 37 cats, 50 gitbucket and 123 library
source paths with errors. Error totals agree exactly; measurement inputs have
not changed in this gate.

## Execution

| check | result |
|---|---|
| `MODE=b tests/slick_run.sh` | `progs=12 ok=12 diff=0 fail=0 runs=3 attempts=36/36` |
| `MODE=a tests/slick_run.sh` | Last separate measure: **RED**, all 12 clients fail to compile; not rerun in this gate |
| `tests/slick_subset.sh` | `subset_files=184 classes=1504 verified=1504 failed=0` |
| `tests/classfile_lint.py` (via subset / slick_run) | `lint_problems=0` |
| `tests/verify_all.sh <slick out>` | `verify_classes=1504 verify_loaded=1504 verify_failures=0 verify_incomplete=0` |

The stronger Slick verification includes the real PostgreSQL 42.7.13 driver,
Scala reflect, and Oracle `ojdbc8_g` 21.23.0.0 (the version pinned by Slick's
`project/Dependencies.scala`). Driver provenance and SHA-256 are recorded in
`/tmp/scala-rs-gate-5adf9c87-codex/oracle-driver.txt`. No class remains incomplete.

## scala/scala corpus (`CORPUS_SIZE=full`, 5324 units)

| kind | pass | fail | skip |
|---|---:|---:|---:|
| `pos` (1859) | **1147** | 367 | 345 |
| `neg` (1405) | **713** | 323 | 369 |
| `run` (2060) | **683** | 824 | 553 |

The complete per-test status reference is
[`baselines/corpus-b969b0d1.tsv`](baselines/corpus-b969b0d1.tsv): 5324 unique
records from scala/scala revision `3f6bdaeafde17d790023cc3f299b81eaaf876ca3`.
The `b969b0d1` gate compared against `corpus-913fb1c0.tsv`: **losses=0,
changes=20**: neg/sammy_expected, pos/context, pos/depmet_1_pos, pos/sammy_exist, pos/sammy_scope, pos/scoping1, pos/scoping3, pos/t0039, pos/t10418_bounds, pos/t1049, pos/t1050, pos/t10792, pos/t11558, pos/t3371, pos/t360, pos/t361, pos/t372, pos/t3861, run/t6443, run/try-catch-unify.

The `913fb1c0` gate compared against `corpus-9cc076f6.tsv`: **losses=0,
changes=3**: pos/implicits-old, pos/t8310, run/fors.

The `9cc076f6` gate compared against `corpus-603b6451.tsv`: **losses=0,
changes=5**: pos/t7532b, pos/t8708, run/exoticnames and run/t9114 now pass;
neg/t1009 now correctly rejects.

The `603b6451` gate compared against `corpus-73f68974.tsv`: **losses=0,
changes=3**: pos/sudoku, pos/t1075 and run/verify-ctor now pass.

The `73f68974` gate compared against `corpus-23031519.tsv`: **losses=0,
changes=38** (27 runtime gains and 11 formerly accepted negative programs now
rejected). Positive counts are unchanged.

The `23031519` gate compared against `corpus-3343f368.tsv`: **losses=0,
changes=11**.

The `3343f368` gate compared against `corpus-647afcf2.tsv`: **losses=0,
changes=0**.

The `647afcf2` gate compared against `corpus-5f0d18c2.tsv`: **losses=0,
changes=1**, the positive gain `delambdafy-patterns`.

The `5f0d18c2` gate compared against `corpus-e355ab70.tsv`: **losses=0,
changes=0**.

The `e355ab70` gate compared against `corpus-a7f05f48.tsv`: **losses=0,
changes=1**, the positive gain `t7264`.

The `a7f05f48` gate compared against `corpus-176629aa.tsv`: **losses=0,
changes=0**.

The `176629aa` gate compared against `corpus-02317f15.tsv`: **losses=0,
changes=1**, the runtime gain `var-arity-class-symbol`.

The `02317f15` gate compared against `corpus-caf8f284.tsv`: **losses=0,
changes=2**, both runtime gains (`sd409`, `t4147`).

The `caf8f284` gate compared against `corpus-5adf9c87.tsv`: **losses=0,
changes=0**.

The `5adf9c87` gate compared against `corpus-172a6525.tsv`: **losses=0,
changes=19**, all fail-to-pass (8 pos, 8 neg, 3 run).

The `172a6525` gate compared against `corpus-88ab9308.tsv`: **losses=0,
changes=1** (`pos/t6033` fail-to-pass). `pos/t6846` remains pass.

The `88ab9308` gate compared against `corpus-218b5340.tsv`: **losses=0,
changes=0**.

The `218b5340` gate compared against `corpus-598ceef1.tsv`: **losses=0,
changes=0**.

The `598ceef1` gate compared against `corpus-9f3cae13.tsv`: **losses=0,
changes=4**, all fail-to-pass: `neg/t6260-named`, `neg/t6260c`,
`run/indylambda-boxing`, and `run/t6260b`.

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
| `cargo test --workspace --release --no-fail-fast` | **307 result rows, 2755 passed, 0 failed** at `b969b0d1` |
| `tests/spec_classfiles.sh` | `tests=37 match=2 differ=26 no_compile=9`, `$sp` scalac=700 scala-rs=0, **LEDGER RED** |

No compiler source, Cargo input, or test fixture changed after the full run.
`cargo clippy --workspace --release` exits zero with **57** individual warning
messages (excluding per-crate generated-warning summaries). Compared with the
saved 57-warning log, there are no additions or removals. Evidence:
`/tmp/scala-rs-dependent-adaptation/harness/clippy-phase.log` and
`/tmp/scala-rs-dependent-adaptation/harness/clippy-compare.json`.
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


## Gate thirty: value classes across generic boundaries

Clean composed `598ceef1` completed `tests/verify_merge.sh` with
`VERDICT=PASS`, `DONE`, no skipped stages, and corpus losses=0 against
`9f3cae13`. Logs: `/tmp/scala-rs-gate-598ceef1-codex/gate.log`.
Main was fast-forwarded to that exact tested commit. The following recording
commit changes only this baseline, the saved ledger and the probe README.

Gitbucket 228/69, cats 163/62, and library 541/123 are unchanged, including
error-message multisets. Slick: zero errors / 1490 classes, all verified,
no lint problems, 12/12 execution with 36/36 attempts. Workspace: 2661 passed,
zero failed (285 rows). Corpus: pos 1109, neg 692, run 637; gains are the four
statuses listed above. All three losses from the rejected gate are recovered.

The four cats lazyZip BuildFrom diagnostics were not explained by this slice.
A cats-shaped AnyVal wrapper instead exposed a silent runtime miscompile:
generic bridges cast the wrapper to its underlying collection and did not box
results. Preserving declaration metadata corrects argument/result adaptation,
generic underlying types and binary accessors. Anonymous erased-name collisions
use expanded implementation names; named collisions are rejected as scalac does.
The first gate exposed receiver boxing and ordinary-function result boundaries;
the final candidate fixes those too. Six focused tests compare rejection and
runtime output against scalac, both ABI modes, and both binary compilation
directions. This is not a claim that all value-class or collection paths are
complete; gitbucket and cats still have the compilation errors recorded above.


## Gate thirty-one: original owners and singleton result erasure

Clean `218b5340`, containing main `73a6ca77`, completed the full merge gate
with `VERDICT=PASS`, `DONE`, no skipped stages, and corpus losses=0,
changes=0 against `598ceef1`. Logs:
`/tmp/scala-rs-gate-218b5340-codex/gate.log`. Main was fast-forwarded to the
exact tested commit. The following record changes only this baseline, the
saved corpus ledger and the probe README.

Cats improves 163/62 -> 159/59: exactly four BuildFrom[Iterable,...]
diagnostics disappear, with no new error-message entries. Gitbucket stays
228/69 and library 541/123, with unchanged error-message multisets. Slick
retains zero errors, 1490 verified classes, zero lint problems and 12/12
execution (36/36 attempts). Workspace: 2662 passed, zero failed (286 rows).
Corpus remains pos 1109, neg 692, run 637. Format checking passes; clippy
exits zero with 59 individual warnings and no additions versus the prior log.

The general BuildFrom-witness hypothesis was wrong. A Scala-signature member
installed on Seq retains the original Iterable declaration and its singleton
receiver result, while a classfile forwarder on AbstractIterable loses that
singleton. Comparing the installation owners missed their shared declaration.
Forwarder filtering now uses the original declaring owner and accounts for
JVM singleton erasure when comparing complete signatures. It does not discard
all competing classfile methods.

The small ArraySeq regression is rejected by the saved pre-fix binary with
the same BuildFrom error. The corrected compiler and real scalac 2.13.16
execute it under java -Xverify:all with byte-identical output; both reject
the incompatible-result fixture. The required member-supply boundary suites
plus the new test pass 535/535. Simpler LazyList probes did not reproduce the
loaded inheritance graph and are not claimed as regression tests. The probe
README preserves that investigation. Gitbucket and cats are not yet fully
compilable; the counts above remain the accepted baseline.


## Rejected binary-parent candidate: `07de211b`

Clean `07de211b`, containing main `57accf2b`, completed the full merge gate
with `VERDICT=PASS`, `DONE`, no skipped stages, and corpus losses=0,
changes=0 against `218b5340`. Logs:
`/tmp/scala-rs-gate-07de211b-codex/gate.log`. The candidate is rejected for
standard-library compilation regression and does not replace the accepted
baseline or main's compiler. The gate script does not enforce this error count.

Library: 543 errors / 123 files, up from 541/123. The only added error-message
entries are two `value fromBitMaskNoCopy is not a member of BitSet$` diagnostics
at immutable/BitSet.scala:373 and mutable/BitSet.scala:390, in readResolve of
nested SerializationProxy classes. Cats stays 159/59 and gitbucket 228/69;
their error-message multisets are unchanged.

Slick: zero errors / 1490 classes, all 1490 verified, zero validation failures
or lint problems, 12/12 execution with 36/36 attempts. Workspace: 2664 passed,
zero failed (288 rows). Corpus remains pos 1109, neg 692, run 637. Saved
candidate ledger: `tests/baselines/corpus-07de211b.tsv`. Format check passes;
clippy exits zero with 59 individual warnings and no additions against the
saved preceding log.

The candidate fixes verified binary inner-class constructor failures across
import aliases and direct/forwarded API paths; its permanent tests compare
stdout bytes and rejection with real scalac. That focused success does not
excuse the source-library regression. The original Slick query probe advances
past its constructor but fails at Rep-to-ProvenShape conversion; no complete
query execution is claimed. This record changes only this baseline and the
candidate ledger; no implementation from the candidate is merged.


## Gate thirty-two: binary parent prefixes and lexical companions

Clean composed `88ab9308`, containing main `a90430ca`, completed the full
merge gate with `VERDICT=PASS`, `DONE`, no skipped stages, and corpus losses=0,
changes=0 against `218b5340`. Logs:
`/tmp/scala-rs-gate-88ab9308-codex/gate.log`. Main was fast-forwarded to that
exact tested commit. The following record changes only this baseline and the
saved corpus ledger; compiler code and test inputs are identical to the gate.

Gitbucket remains 228/69, cats 159/59 and library 541/123. All three error-message
multisets match the accepted baseline. Both BitSet errors introduced by rejected
07de211b are gone. Slick retains zero errors, 1490 verified classes, zero lint
problems, and 12/12 executed programs with 36/36 attempts. Workspace: 2665 passed,
zero failed (288 rows). Corpus remains pos 1109, neg 692, run 637 across 5324
records. Format checking passes; clippy has 59 existing warnings with no additions.

Binary inner-class parent constructors now retain stable outer instances across
import aliases, direct and forwarded singleton API paths, and API values. The
fixtures execute under JVM verification and compare stdout bytes with real
scalac, including omitted parentheses and renamed aliases; both compilers reject
incompatible arguments. The full gate exposed lexical scope pollution: qualified
companion lookup installed a bare name and shadowed an enclosing same-name object.
A small source reproduces that rejection on 07de211b while scalac and the accepted
baseline compile it. The correction keeps scope insertion in the unqualified
caller; its execution test matches scalac in both ABI modes. Focused tests total
536 passed, zero failed.

This fixes a silent constructor VerifyError, not the remaining gitbucket Shape
family. The Slick query probe advances past construction and exposes an unresolved
Rep-to-ProvenShape conversion; complete query execution is still unproven. Neither
gitbucket nor cats is yet fully compilable.


## Rejected conversion-witness candidate: `9ad530b2`

Clean `9ad530b2`, containing main `0afdfd22`, completed the full merge gate
with `VERDICT=FAIL`, `DONE`, and no skipped stages. Logs:
`/tmp/scala-rs-gate-9ad530b2-codex/gate.log`. No implementation from this
candidate is merged, and the accepted baseline remains `88ab9308`.

Gitbucket improves 228/69 -> 223/68: five update-overload diagnostics involving
Date disappear, with no added error-message entries. Cats stays 159/59 and
library 541/123, with unchanged error-message multisets. Slick keeps zero errors,
1490 verified classes, zero lint problems, and 12/12 runtime programs with
36/36 attempts. Workspace: 2666 passed, zero failed (289 rows). Format passes;
clippy has 59 existing warnings and no additions.

Corpus totals are unchanged (pos 1109, neg 692, run 637), but losses=1,
changes=2 against `88ab9308`: pos/t6033 improves and pos/t6846 regresses.
The latter reports Carb[Nothing[Nothing]] required Carb[x.type], in a
higher-kinded implicit conversion with singleton targets and subtype evidence.
The equal totals do not excuse that pass-to-fail transition. Candidate ledger:
`tests/baselines/corpus-9ad530b2.tsv`.

Focused tests pass 535/535, including binary Shape witness runtime comparisons
and rejection of an incompatible result. The explicitly typed Slick query
probe produces 139 SQL bytes matching scalac under JVM verification. The gate
regression requires further inference investigation before another gate. This
record changes only this file and the candidate ledger, not main's compiler.


## Gate thirty-three: conversion witnesses and singleton inference

Clean composed `172a6525`, containing main `629fcf35`, completed the full
merge gate with `VERDICT=PASS`, `DONE`, no skipped stages, and losses=0,
changes=1 against `88ab9308`. Logs:
`/tmp/scala-rs-gate-172a6525-codex/gate.log`. Main was fast-forwarded to the
exact tested commit. The following recording commit changes only this baseline
and the saved corpus ledger.

Gitbucket improves 228/69 -> 223/68: the same five Date-related update-overload
diagnostics from the rejected gate disappear, with no new error-message entries.
Cats stays 159/59 and library 541/123, with unchanged diagnostics. Slick retains
zero errors, 1490 verified classes, no lint problems, and 12/12 execution with
36/36 attempts. Workspace: 2667 passed, zero failed (289 rows). Corpus: pos 1110,
neg 692, run 637. The only gain is pos/t6033; pos/t6846 has recovered. Format
passes; clippy retains 59 existing warnings with no additions.

A typed Slick projection now receives its ProvenShape conversion and generates
SQL byte-identical to scalac under JVM verification. The initial result-inference
hypothesis alone did not fix it: binary witness completion was missing. Once
completion exposed candidates, a negative probe found that result-derived type
arguments also had to be retained when validating evidence. Otherwise Rep[Int]
was wrongly accepted as Proven[String]. Both corrections are tested together.

The rejected gate then exposed an older hidden inference defect: a higher-kinded
parameterless result inferred Nothing[Nothing] against a singleton target.
Reading the singleton's underlying constructor infers List and Int; ordinary
adaptation still requires a valid narrowing conversion. The new runtime case
prints 7/8/8 with scalac and scala-rs, and both reject narrowing without that
conversion. Focused tests pass 536/536. These tests use the real jar; the private
runtime does not provide the required <:< type.

Unannotated abstract overrides remain a distinct silent miscompile. A seven-line
source at `/tmp/scala-rs-abstract-result/Probe.scala` compiles on both the saved
accepted binary and this candidate, then throws ClassCastException; scalac runs
and prints 8. The typer only borrows inherited result types with an explicit
override modifier, missing legal implementations of abstract methods. This is
pre-existing and explains why unannotated Table projections remain unfinished.
Gitbucket and cats still have the errors recorded above and are not fully compiled.


## Rejected abstract-result candidate: `0611af2f`

Clean `0611af2f`, containing main `a900fb05`, completed the full merge gate
with `VERDICT=FAIL`, `DONE`, and no skipped stages. Logs:
`/tmp/scala-rs-gate-0611af2f-codex/gate.log`. No implementation from this
candidate is merged; the accepted baseline remains `172a6525`.

Slick regresses from zero errors to 4 errors in 2 files, producing no classes
in the full 184-file compile. slick_run fails at compilation, so its runtime
programs are not validated. The subset shrinks to 111 files / 780 classes
(verified 780, failed 0, lint problems 0), which fails the 1490-class gate.
Library regresses 541/123 -> 576/124. Cats stays 159/59 and gitbucket 223/68,
with no changes to their error-message multisets. Workspace passes 2668 tests,
zero failed (290 rows). Format passes; focused clippy had 59 existing warnings
with no additions.

Corpus totals: pos 1110, neg 693, run 637. The full 5324-record comparison
reports losses=5, changes=11. Losses: neg/t6276; pos/t7212, pos/t7668,
pos/t8146b; run/t7912. Gains: neg/t4612, neg/val_infer; pos/t3079,
pos/t6925b, pos/t7200b; run/t7200. The equal positive/run totals and net one
negative gain hide five individual regressions. Saved candidate ledger:
`tests/baselines/corpus-0611af2f.tsv`.

The Slick failures include inherited R/RU/T parameters retaining different
declaring owners in JdbcActionComponent.scala and SimpleFunction.scala. Similar
owner mismatches account for many library additions. Simple anonymous-class,
cast, nested-profile and polymorphic-factory controls all compile on both the
accepted and candidate binaries; they do not reproduce the full-input fault.
Those controls and logs are in `/tmp/scala-rs-abstract-regression`. The next
investigation should trace actual signature origins in the failing input and
also address the negative-test acceptance regression before another gate.

The focused source/binary/runtime successes remain valid, but they do not
establish safety of the broadened inherited-result and lazy-completion paths.
This recording commit changes only this file and the candidate ledger; main's
compiler remains unchanged.


## Rejected repaired abstract-result candidate: `c51614e2`

Clean `c51614e2` combines repair commit `eb47896d` with main `34d7f807`.
The full, unskipped gate completed with `VERDICT=FAIL` and `DONE`:
`/tmp/scala-rs-gate-c51614e2-codex/gate.log`. No compiler changes from this
candidate are merged; accepted baseline remains `172a6525`.

Gitbucket regresses from 223 errors / 68 files to 234 / 70. Cats improves
159 / 59 -> 158 / 59; the library improves 541 / 123 -> 475 / 120.
The library diagnostic location/message comparison has zero additions and
66 removals. Gitbucket additions include block-local values incorrectly
inheriting same-named ancestor result types under `-Xsource:3-cross`.
A six-line reproduction and real-scalac comparison are saved under
`/tmp/scala-rs-abstract-regression/local-shadow/`. The accepted compiler
already emits a spurious override diagnostic for that reduced example;
the candidate additionally imposes the ancestor's type on the local value.

Slick: 184 files, zero errors, 1492 classes; verified 1492, failed 0,
lint problems 0. Runtime MODE=b passes 12/12 programs and 36/36 attempts.
The two extra classes implement Ordering SAMs for ScalaBaseType and
ScalaOptionType; an accepted-class runtime probe throws ClassCastException,
while candidate output matches the real-scalac client with published Slick.
Details and fixtures are committed in the candidate's abstract-result probes.
Workspace: 2685 passed, zero failed (290 rows). Format passes.

Corpus: 5324 rows; pos 1116 / 398 / 345, neg 696 / 340 / 369,
run 640 / 867 / 553 (pass / fail / skip). Against `172a6525`, there are
15 status changes: 14 gains and one loss, `neg/t11136_override_conflict`.
The five losses of the previous `0611af2f` candidate are recovered, but
this new acceptance regression and the gitbucket increase prohibit merging.
Candidate ledger: `tests/baselines/corpus-c51614e2.tsv`.

This recording commit changes only BASELINE and the raw candidate ledger.
The next repair must distinguish local declarations from template members
and restore rejection of conflicting overrides before another composed gate.


## Rejected inherited-implementation candidate: `4d3bc103`

Clean `4d3bc103` combines `3d1dcfa1` with main `617b6519`. Its full gate
completed without skips, with `VERDICT=FAIL` and `DONE`:
`/tmp/scala-rs-gate-4d3bc103-codex/gate.log`. The accepted compiler baseline
remains `172a6525`; no implementation from this candidate is merged.

Compile measures improve: gitbucket 223/68 -> 217/67, cats 159/59 -> 158/59,
library 541/123 -> 474/120. All three error-message multisets have no additions.
Slick compiles 184 files with zero errors and 1492 classes; subset reports
verified 1492, failed 0, lint problems 0. MODE=b executes 12/12 programs,
36/36 attempts. Format passes. Clippy has 57 existing diagnostic warnings,
unchanged from the previous repair checkpoint.

Workspace: 2682 passed, six failed (290 rows). Failures are
`seqfn_fixture_dual_run`, `deep_diamond_hierarchy_terminates`,
`diamond_cost_does_not_double_per_level`, `fixtures_vf_nothing`,
`fixtures_vf_nothing_lib`, and `vf_nothing_bridges_match_scalac`.
The seqfn test rejects a PartialFunction[Int, Int] as though it required
PartialFunction[Any, Int]. The diamond comparison takes 0.195 seconds at 22
levels but 24.995 seconds at 30; the 34-level case exceeds 60 seconds.
The three verifyfail tests expose a missing String-returning bridge for a
Nothing-returning implementation.

An additional, stronger initialization sweep of the exact Slick output fails:
`verify_classes=1492 verify_failures=1 verify_loaded=1490 verify_incomplete=1`.
QueryInterpreter.StructValue.apply(Object): Object calls apply(Int): Object
without unboxing its argument, producing VerifyError. TimestamptzConverter
is incomplete because the Oracle JDBC driver is absent; PostgreSQL's real
cached driver was provided. The ordinary subset check and 12 runtime programs
missed this bad bridge. Logs: `verify-all.log`, `verify-all-verbose.log`,
`structvalue-javap.txt`; the class output is retained as `slick-classes/` in
the gate directory. No verification success is claimed from the subset alone.

Corpus: 5324 rows, losses=3, changes=20 (17 gains). Counts (pass/fail/skip):
pos 1118/396/345, neg 696/340/369, run 639/868/553. New losses against the
accepted reference: neg/t9717, pos/t13013, run/transform. The prior loss
neg/t11136_override_conflict is recovered. Raw ledger:
`tests/baselines/corpus-4d3bc103.tsv`.

Next repairs must constrain hierarchy completion without reintroducing
exponential traversal or changing existing collection signatures, preserve
Nothing bridges, and adapt primitive arguments in inherited bridges. Each
loss and existing failing test remains a gate requirement. This record changes
only BASELINE and the candidate ledger; main's implementation is unchanged.


## Rejected constructor-scope candidate: `62fab7e4`

On 2026-09-10, clean `62fab7e4` combined repair commit `305d34c5` with
main `a0f68dee`. Its full gate completed with no skipped stages,
`VERDICT=FAIL` and `DONE`:
`/tmp/scala-rs-gate-62fab7e4-codex/gate.log`. No candidate implementation
is merged; the accepted compiler baseline remains `172a6525`.

Workspace passes 2693 tests, zero failed (290 rows). Format passes; release
workspace clippy retains 57 diagnostic warnings. Gitbucket improves from
223/68 to 217/67 and cats from 159/59 to 158/59, with no added error-message
entries. Slick compiles 184 files with zero errors and 1492 classes; subset
reports verified 1492, failed 0, lint problems 0. The stronger initialization
sweep of the retained exact classes reports verify_classes=1492,
verify_failures=0, verify_loaded=1491, verify_incomplete=1. Oracle's absent
JDBC driver still prevents initialization of TimestamptzConverter; this is
not reported as complete verification. Logs: `verify-all.log`, with class
output retained as `slick-classes/`.

Two measurements encountered damaged temporary caches. Library reports
656 errors in 142 files, but its Java classpath contains only one of the
33 classes required from the released jar: BoxedUnit, Statics and 30 others
are missing. `java-cache-audit.json` records the inventory. This is not a
valid comparison with the accepted 541/123; regenerate the cache before
attributing the increase to compiler code. Slick execution fails all 12
client compilations (zero runtime attempts) because the reused scalac-side
class output lacks classes including BasicActionComponent and SqlActionComponent.
The client compile diagnostics are preserved in `slick-program-logs/`.
The next gate must force regeneration with REUSE_SCALAC=0. Broken Scala and
Slick source checkouts were preserved and replaced with fresh clones pinned
to their scripts' exact revisions before this gate. The fresh corpus path is
`/tmp/scala-rs-corpus-20260910-codex` at
3f6bdaeafde17d790023cc3f299b81eaaf876ca3.

Separately from those environment failures, the full 5324-row corpus reports
losses=3, changes=22 (19 gains). Counts (pass/fail/skip): pos 1116/398/345,
neg 699/337/369, run 640/867/553. The prior losses neg/t9717, pos/t13013 and
run/transform recover. New losses are pos/t1798 (companion-private access
from auxiliary constructor arguments), pos/t12233 and neg/t12233 (class
context-bound evidence required on auxiliary constructors). These compiler
regressions independently prohibit merging. Raw candidate ledger:
`tests/baselines/corpus-62fab7e4.tsv`.

Focused validation before the gate passed 71 related tests and 534 boundary
tests. Real-scalac probes cover all four t9717 rejection sites separately;
a new JVM runtime probe also exposed and repaired qualified module-this
loading from an uninitialized constructor receiver. The remaining three
corpus regressions and damaged caches must be repaired before another gate.
This recording commit changes only BASELINE and the raw candidate ledger.


## Gate thirty-four: inherited results and constructor evidence

Clean `5adf9c87`, combining `7058f73e` with main `104a217e`, completed the
full unskipped gate on 2026-09-10 with `VERDICT=PASS`, `DONE`, corpus losses=0
and changes=19 against accepted `172a6525`. Logs:
`/tmp/scala-rs-gate-5adf9c87-codex/gate.log`. Main was fast-forwarded to the
exact tested commit. The recording commit changes only this baseline, its
raw corpus ledger and the permanent process rules in `.agent-brief.md`.

Gitbucket improves 223/68 -> 217/67, cats 159/59 -> 158/59, and the library
541/123 -> 460/119. Anonymous-class serial numbers are normalized when
comparing diagnostic messages: zero additions, respectively 6, 1 and 81
removals. The three raw library message changes are unchanged BuildFrom
errors at unchanged locations with shifted anonymous-class numbers. The
comparison is saved as `diagnostic-comparison.json` in the gate directory.
Slick retains zero compile errors and produces 1492 classes. The two extra
classes relative to the accepted 1490 implement Ordering SAMs; the retained
probes show the old Function2 cast failing and the new behavior matching
published Slick with a real-scalac client.

Slick MODE=b passes 12/12 programs and 36/36 execution attempts. Subset and
lint pass all 1492 classes. Strong initialization verification, now using
real PostgreSQL and Oracle JDBC drivers, passes all 1492 classes with zero
failures and zero incomplete classes. Exact output is retained in
`slick-classes/`, with `verify-all.log`. The broken Java support cache was
regenerated and the scalac-side Slick reference was forcibly rebuilt before
accepting these measurements. The fresh source checkouts remain pinned to
the scripts' required revisions.

Workspace passes 2695 tests, zero failed (290 rows). Format passes and
clippy has no new warnings. Corpus totals (pass/fail/skip): pos 1118/396/345,
neg 700/336/369, run 640/867/553. All 5324 records are saved in
`tests/baselines/corpus-5adf9c87.tsv`. All losses from the four rejected
abstract-result candidates have recovered. The 19 gains are 8 positive,
8 negative and 3 runtime cases, with no previously passing case lost.

The slice supplies inherited abstract result expectations without requiring
an explicit override modifier, while preserving narrower inferred results.
It also fixes owner/type-parameter substitution, recursive implicit evidence
instantiation, self-typed this results, inherited bridge adaptation, block
scope re-entry and constructor argument scopes. Fresh class-bound evidence
on auxiliary constructors replaces illegal reads from uninitializedThis;
its JVM parameter order and context/view-bound execution match scalac.
Lexical access privileges remain available when the constructing receiver
is unavailable. Source-3 InferOverride support remains explicitly partial,
and gitbucket/cats are still not fully compiled.

The user's process improvements are now permanent rules: run related tests
and all recorded regressions before a full gate, check pinned sources/jars/
cache contents before launch, and consolidate monitoring around state changes
and the same live process handle. Operational checks exercised during this
run inspected four sources, 121 jars, 33 Java support classes and 1498 scalac
reference classes. Negative controls detected all 32 missing Java classes and
1172 missing reference classes in the preserved broken caches. Monitoring
controls detected both a failing test and DONE. These process checks are
separate evidence, not claims that they were stages of this compiler gate.


## Gate thirty-five: lexical scopes for early forward completion

Clean `caf8f284`, based on main `8521dc1f`, passed the complete unskipped
merge gate with `VERDICT=PASS`, `DONE` and corpus losses=0, changes=0.
Logs: `/tmp/scala-rs-gate-caf8f284-codex/gate.log`. Main was fast-forwarded
to the exact tested commit. The following recording commit changes only
BASELINE and the raw corpus ledger; compiler sources and tests are identical.

Gitbucket improves 217/67 -> 202/64: thirteen ambiguous constructor errors,
one readLine-on-Any error and one Releasable[Any] error disappear. No error
messages are added. Cats remains 158/59 and the library remains 460/119,
with identical error-message multisets. Slick compiles all 184 sources with
zero errors and 1492 classes. MODE=b passes 12/12 programs, 36/36 attempts;
subset verifies all 1492 classes with zero failures and lint problems.
Strong initialization verification with the real PostgreSQL and Oracle jars
loads all 1492 retained classes, zero failed and zero incomplete.

Workspace: 2696 passed, zero failed, 291 result rows. Format passes. Release
workspace clippy retains the same 57 warning messages, with none added.
Corpus: 5324 records; pos 1118/396/345, neg 700/336/369 and run 640/867/553
(pass/fail/skip), identical to `5adf9c87`. Raw ledger:
`tests/baselines/corpus-caf8f284.tsv`.

The initial constructor-overload hypothesis was corrected by measurement.
An anonymous class in a parent constructor argument forced an unannotated
member in a later unit before its imports had a scope snapshot. File lookup
failed, Error was cached, and later constructors appeared ambiguous. The
header pass now preserves lexical scopes for pending values and methods as
it already did for aliases. No overload refusal was suppressed.

The two-unit reduction fails on the accepted pre-fix compiler. Real scalac
2.13.16 and the repaired compiler both accept both file orders, reject the
wrong-result-type case, and produce byte-identical stdout under -Xverify:all.
Before the full gate, 89 related tests, 534 boundary tests and 17 historical
corpus cases passed their comparisons (corpus changes=0, losses=0). Preflight
checked four pinned source trees, 121 jar archives, 33 Java support classes
and 1498 reference classes. Main remains an incomplete Scala compiler;
gitbucket and cats do not yet compile fully.


## Gate thirty-six: inherited factory redirection

Clean `02317f15`, based on main `4ff20d52`, passed the complete unskipped
merge gate with `VERDICT=PASS`, `DONE`, corpus losses=0 and changes=2.
Logs: `/tmp/scala-rs-gate-02317f15-codex/gate.log`. Main was fast-forwarded
to the exact tested commit. The recording commit changes only this baseline
and `tests/baselines/corpus-02317f15.tsv`.

Gitbucket improves 202/64 -> 192/63, with no added diagnostics. Four String/K
mismatches, three DirCacheEntry/V mismatches and three downstream errors
are removed. Cats remains 158/59 and the library remains 460/119, with
identical diagnostic multisets. Slick remains errors=0, classes=1492,
subset verified=1492, failed=0 and lint_problems=0. MODE=b passes 12/12
programs and 36/36 attempts. Strong initialization verification of the
retained classes, using the real PostgreSQL and Oracle drivers, loads all
1492 with zero failures and zero incomplete classes.

Workspace passes 2697 tests, zero failures, 292 result rows. Format passes;
release workspace clippy retains exactly 57 warning messages, none added.
Corpus: 5324 records; pos 1118/396/345, neg 700/336/369, run 642/865/553
(pass/fail/skip). The two gains are run/sd409 and run/t4147. No pass is lost.

The failure was not in Map's key checks. Loading MapFactory.Delegate.apply
made both its inherited method and the prelude companion method visible.
The omitted `.apply` redirect used an unfiltered lookup; unable to choose
one candidate, it left explicit type arguments on the module and the later
call returned Map[K,V]. The redirect now uses the same inherited override
filter as explicit member selection. No tuple representation or overload
signature was changed. The one-file warming probe fails on the accepted
pre-fix compiler and runs identically to scalac after the repair. Tests
compare explicit and omitted apply, reject wrong type arguments/keys, and
compare stdout under JVM -Xverify:all.

Before the gate, 535 new/boundary tests and 97 additional related tests
passed. Seventeen historical corpus cases had losses=0, changes=0. Source,
jar and cache preflight passed: four sources, 121 jars, 33 Java classes and
1498 reference classes. Gitbucket and cats still do not compile fully.


## Rejected partial-factory inference candidate: `41cd9040`

Clean `41cd9040`, based on main `ac22b8f0`, completed the full unskipped
merge gate with `VERDICT=FAIL` and `DONE`. Logs:
`/tmp/scala-rs-gate-41cd9040-codex/gate.log`. No candidate implementation
is merged; the accepted compiler baseline remains `02317f15`.

Cats improves 158/59 -> 152/58 and gitbucket 192/63 -> 191/63. The library
remains 460/119. Slick regresses from zero errors and 1492 classes to five
errors in two files, with zero classes from the full compile. Two errors
are Resource.allocated results left as implicit method types in BasicBackend;
three are collection inference failures in RewriteJoins. Slick execution
cannot compile, so no runtime success is claimed. The subset reaches only
82 of 184 files and 638 classes (verified 638, failed 0, lint problems 0).
This is not complete Slick validation and independently prohibits merging.

Workspace passes 2699 tests, zero failed (293 result rows); format passes.
Corpus compares all 5324 identities with losses=0 and changes=1. Counts
(pass/fail/skip): pos 1118/396/345, neg 700/336/369, run 643/864/553.
The only gain is run/var-arity-class-symbol. Raw candidate ledger:
`tests/baselines/corpus-41cd9040.tsv`.

Before the gate, 22 related tests and 534 supply-boundary tests passed.
Seventeen historical corpus identities were unchanged against the accepted
ledger. Preflight checked four pinned source trees, 121 jar archives,
33 Java support classes and 1498 scalac reference classes. Monitoring used
one owned live gate handle through DONE. The tested worktree remained clean.

The candidate preserves factory receiver parameters until the returned apply
can constrain them, and loads binary nested apply members through classfile
completion. Source/binary runtime probes match scalac 2.13.16 and reject
missing evidence and conflicting constraints. Preserving lower-bounded
result parameters also exposes incomplete call-site inference previously
hidden by declaration-time pinning. That proposed cause must be confirmed
against the five Slick failures before another gate. The first standalone
Resource reduction also fails on the accepted compiler, so it is not yet
a regression-specific reproduction. Evidence is under
`/tmp/scala-rs-partial-regression-41cd9040`.

A separate real-scalac runtime probe with a user-defined App[F[_]] exposed
an erroneous scala.App initialization and IncompatibleClassChangeError.
It remains unrepaired; evidence is in `/tmp/scala-rs-partial-probe`.
This recording commit changes only BASELINE and the candidate ledger;
main's compiler sources and tests remain unchanged. Clippy was not rerun
for this rejected candidate; no new clippy-success claim is made.


## Rejected repaired partial-factory candidate: `ad190a6c`

Clean `ad190a6c` combines repair commits `429b1ec5` and `a04c1e39` with
main `a254315c`. Its complete unskipped gate reached `VERDICT=FAIL` and
`DONE`: `/tmp/scala-rs-gate-ad190a6c-codex/gate.log`. No candidate code is
merged; the accepted compiler baseline remains `02317f15`.

The five Slick errors from `41cd9040` are repaired. All 184 files compile
with zero errors and 1492 classes. MODE=b passes 12/12 programs and 36/36
attempts. Subset verification covers all 1492 classes, failed 0 and lint
problems 0. Strong initialization verification of the retained exact output,
with real PostgreSQL and Oracle drivers, loads 1492 classes, failed 0 and
incomplete 0. Classes and `verify-all.log` are in the gate directory.

Cats improves 158/59 -> 152/58, gitbucket 192/63 -> 191/63; the library
remains 460/119 with identical diagnostics. Diagnostic locations have no
additions: six cats locations and one gitbucket location disappear. Some
remaining messages change (Applicative[F] to Applicative[F0], three Query
application failures to result-type mismatches, and one Set argument's type).
Both message and location comparisons are saved in the gate directory;
this is not claimed as an unchanged diagnostic-message multiset.

Workspace passes 2700 tests, zero failed (293 rows). Format passes. Release
workspace clippy has the same 57 diagnostic warnings, with none added.
All 5324 corpus identities compare, with losses=1 and changes=2. Totals
(pass/fail/skip): pos 1118/396/345, neg 699/337/369, run 643/864/553.
The gain is run/var-arity-class-symbol; the loss is
neg/typevar_derive_alias, which the candidate incorrectly accepts. Raw ledger:
`tests/baselines/corpus-ad190a6c.tsv`.

The regression distinguishes binding a parameterless result to a value from
using it directly as a selection qualifier. The corpus defines
`Sq[+T].toSt[B >: T]: St[B]` with invariant St and `St.map[U](T => U)`.
Scalac accepts `val st = ts.toSt; st.map(x => x)`, but rejects the direct
`ts.toSt.map(x => x)` forms, with and without an alias. The new eager
lower-bound inference makes the direct forms pass as well. This qualifier
context and both rejection sites must become focused prerequisites before
another gate; no workaround should weaken or remove the corpus test.

Before launch, 557 related/boundary tests passed, and 17 historical corpus
identities had losses=0 and changes=0 on the composed tree. Preflight passed
four pinned sources, 121 jars, 33 Java support classes and 1498 scalac
reference classes. One owned process handle was monitored through DONE.
This recording commit changes only BASELINE and the candidate ledger;
main's compiler sources and tests remain unchanged.


## Gate thirty-seven: partial factory receiver inference

Clean `176629aa`, combining repair `b7925f4a` with main `6a348052`, passed
all unskipped gate stages with `VERDICT=PASS`, `DONE`, corpus losses=0 and
changes=1 against `02317f15`. Logs:
`/tmp/scala-rs-gate-176629aa-codex/gate.log`. Main was fast-forwarded to the
exact tested commit; this recording commit changes only BASELINE and the
raw corpus ledger. No compiler source or fixture changed after the gate.

Cats improves 158/59 -> 152/58 and gitbucket 192/63 -> 191/63. The library
remains 460/119. Diagnostic messages are identical to rejected `ad190a6c`:
relative to the accepted baseline, no error location is added, six cats
locations and one gitbucket location disappear. Several remaining messages
change as documented in that rejection, so this is not a claim that the
error-message multiset has no additions. Slick compiles all 184 files with
zero errors and 1492 classes. MODE=b passes 12/12 programs, 36/36 attempts;
subset verifies all 1492 classes with zero failures and lint problems.
Strong initialization verification with real PostgreSQL and Oracle drivers
loads all 1492 retained classes, zero failed and zero incomplete.

Workspace: 2701 passed, zero failed, 293 rows. Format passes. Release
workspace clippy has the same 57 diagnostic warnings, with no additions.
Corpus: 5324 identities; pos 1118/396/345, neg 700/336/369, run 643/864/553
(pass/fail/skip). The only gain is run/var-arity-class-symbol. The prior
loss neg/typevar_derive_alias recovers. Raw ledger:
`tests/baselines/corpus-176629aa.tsv`.

The slice infers receiver parameters of partially applied factories from
arguments and expected results before implicit search, and loads nested
binary apply members through the normal classfile path. Lower bounds remain
on binary declarations and are inferred at use sites. Implicit-only results
and parameterless values both infer real lower bounds without freezing
factory receivers before explicit or omitted apply. Dynamic classification
preserves the same callee context. Selection qualifiers defer value-position
minimization, retaining scalac's rejection of direct invariant-result map
chains, including aliases.

The last regression is covered by independent plain and alias rejection
probes: real scalac rejects both; the saved pre-fix candidate accepts both;
the final candidate rejects both. Binding the receiver to a local value is
accepted and executes with byte-identical stdout under JVM -Xverify:all.
Factory tests also compare source and real-scalac-built binary libraries,
exercise explicit/omitted apply, and independently reject invalid bounds,
missing evidence and explicit wide results assigned to narrow types.

Before the gate, 558 related/boundary tests passed, and 18 historical corpus
identities matched the accepted ledger with changes=0 and losses=0. Source,
jar and cache preflight passed four pinned checkouts, 121 jars, 33 Java
support classes and 1498 scalac reference classes. Monitoring retained one
live handle through DONE. Gitbucket and cats still do not fully compile.
The separately reproduced user-defined App initialization collision and
class-directory implicit metadata loss remain open investigations.


## Gate thirty-eight: App and DelayedInit runtime identities

Clean `a7f05f48`, based on main `6409e88f`, passed the complete unskipped
gate with `VERDICT=PASS`, `DONE`, corpus losses=0 and changes=0. Logs:
`/tmp/scala-rs-gate-a7f05f48-codex/gate.log`. Main was fast-forwarded to
that exact tested commit. The recording commit changes only BASELINE and
the raw corpus ledger; all compiler sources and tests match the gated tree.

All four compile measures are unchanged: gitbucket 191/63, cats 152/58,
library 460/119; Slick 184 sources, zero errors and 1492 classes. The three
error-message multisets exactly match accepted `176629aa`. Slick MODE=b
passes 12/12 programs and 36/36 attempts. Subset validates all 1492 classes,
failed 0 and lint problems 0. Strong initialization verification with the
real PostgreSQL and Oracle drivers loads all 1492 retained classes, failed
0 and incomplete 0. Class output and `verify-all.log` are retained in the
gate directory.

Workspace: 2702 passed, zero failed, 294 rows. Format passes. Release
workspace clippy retains exactly 57 warning messages with no additions.
All 5324 corpus identities are unchanged: pos 1118/396/345,
neg 700/336/369, run 643/864/553 (pass/fail/skip). Raw ledger:
`tests/baselines/corpus-a7f05f48.tsv`.

The backend selected Scala initialization protocols by the simple parent
names App and DelayedInit. A user-defined same-named trait therefore caused
calls to scala.App.$init$ or a nonexistent delayedInit method. The accepted
pre-fix compiler compiles both reductions but throws IncompatibleClassChangeError
for App and NoSuchMethodError for DelayedInit. The repair compares fully
qualified JVM identities throughout the parent traversal. New fixtures cover
indirect user App inheritance, anonymous implementations, same-named
DelayedInit, and real scala.App and scala.DelayedInit. Their outputs match
real scalac byte for byte under JVM -Xverify:all, in both real-library and
private-runtime modes. A wrong assignment to scala.App remains rejected.
The independent DelayedInit reduction is in `/tmp/scala-rs-app-identity-probe`.

Before the gate, the new runtime test and 397 existing E2E tests passed;
all 18 historical corpus identities matched the baseline. Preflight checked
four pinned source trees, 121 jars, 33 Java support classes and 1498 scalac
reference classes. One owned live handle was monitored through DONE.
This fixes the App collision left open by gate thirty-seven despite moving
none of the four compile counts or corpus totals. Class-directory signature
metadata loss remains an open investigation; gitbucket and cats are incomplete.

## Gate thirty-nine: complete directory Scala signatures

Clean `e355ab70`, based on main `26165bca`, passed the complete unskipped
merge gate with `VERDICT=PASS`, `DONE`, corpus losses=0 and changes=1.
Logs: `/tmp/scala-rs-gate-e355ab70-codex/gate.log`. Main was fast-forwarded
to the exact tested commit. The recording commit changes only BASELINE and
the raw corpus ledger; compiler sources and tests match the gated tree.

All four compile measures are unchanged: gitbucket 191/63, cats 152/58,
library 460/119; Slick 184 sources, zero errors and 1492 classes. The three
error-message multisets exactly match accepted `a7f05f48`. Slick MODE=b
passes 12/12 programs and 36/36 attempts. Subset validates all 1492 classes,
failed 0 and lint problems 0. Strong initialization verification with the
real PostgreSQL and Oracle drivers loads all 1492 retained classes, failed
0 and incomplete 0. Class output and `verify-all.log` remain in the gate directory.

Workspace: 2703 passed, zero failed, 295 rows. Format passes. Release
workspace clippy retains exactly 57 warning messages with no additions.
Corpus: pos 1119/395/345, neg 700/336/369, run 643/864/553 (pass/fail/skip).
All 5324 identities are present; only pos/t7264 changes from fail to pass.
Raw ledger: `tests/baselines/corpus-e355ab70.tsv`.

Directory classpath scanning supplied shallow Scala declarations. Existing
member names then bypassed full signature loading, losing implicit clauses
and generic parent arguments. Selection now completes pending binary Scala
signatures, including full parents and ancestor completion. The hypothesis
was confirmed with fresh real-scalac classfiles: the same bytes behaved
differently in a directory and a jar. Merely adopting own members was
insufficient; inherited generic parents also required completion.

The new dirsig test compiles a library with real scalac, packages the same
classfiles into a jar, and checks both classpath forms against scalac for
acceptance and rejection. Positive programs run under -Xverify:all and their
stdout matches byte for byte. The accepted pre-fix binary rejects the valid
directory program and incorrectly accepts flattened argument lists. The
repair handles inherited type arguments, lower bounds and implicit evidence;
missing evidence and flattened clauses remain rejected in both forms.
Evidence: `/tmp/scala-rs-classdir-probe/`.

Before this gate, 542 focused and related tests passed, and all 18 recorded
corpus regression identities were unchanged. Preflight verified four pinned
source trees, 121 jars, 33 Java support classes and 1498 reference classes.
One owned gate execution was followed through DONE with the consolidated
phase/failure watcher; no gate step was restarted or skipped.

## Gate forty: immutable Map key types

Clean `5f0d18c2`, based on main `a0230911`, passed the complete unskipped
merge gate with `VERDICT=PASS`, `DONE`, corpus losses=0 and changes=0.
Logs: `/tmp/scala-rs-gate-5f0d18c2-codex/gate.log`. Main was fast-forwarded
to the exact tested commit. The recording commit changes only BASELINE and
the raw corpus ledger; compiler sources and tests match the gated tree.

All compile measures and their error-message multisets are unchanged from
accepted `e355ab70`: gitbucket 191/63, cats 152/58, library 460/119; Slick
184 sources, zero errors and 1492 classes. Slick MODE=b passes 12/12 programs
and 36/36 attempts. Subset validates 1492 classes with zero failures and lint
problems. Strong initialization verification with the real PostgreSQL and
Oracle drivers loads all 1492 classes, zero failures and zero incomplete.
Class output and verify-all.log are retained under the gate directory.

Workspace: 2704 passed, zero failed, 296 rows. Format passes. Release
workspace clippy retains 57 warning messages with no additions. Corpus:
pos 1119/395/345, neg 700/336/369, run 643/864/553 (pass/fail/skip), all
5324 identities unchanged. Raw ledger: `tests/baselines/corpus-5f0d18c2.tsv`.

The immutable Map prelude admitted Any as the key for apply, get, contains,
updated and getOrElse. The real-scalac bidirectional probe confirmed this
hypothesis for all five members: the accepted pre-fix compiler accepts Int
keys on Map[String, String], whereas scalac rejects each independently.
The declarations now use K. Existing value-widening type parameters and
lower bounds remain intact. The mapkey fixture compares correct String and
Int keys, Map[Any, String], widened updated/getOrElse values and implicit
key conversions against scalac at runtime with -Xverify:all and byte-exact
stdout. Each wrong-key member is independently checked against both compilers.
Evidence: `/tmp/scala-rs-mapkey-probe/`, including the rebuilt pre-fix binary's
five false acceptances in before.json.

Before the gate, 549 new and related tests passed; 18 recorded corpus
regression identities had no changes. Preflight checked four pinned source
trees, 121 jars, 33 Java support classes and 1498 reference classes. The gate
was run once and its owned execution followed through DONE. Compile metrics
and corpus counts do not expose the repaired false acceptances.

A read-only follow-up investigated gitbucket's RepositoryResolver and
ReceivePackFactory kind diagnostics. The actual JGit jar declares generic
interfaces, and isolated real-scalac/scala-rs inheritance probes both pass,
including preceding GitServlet and ReceivePack subclasses. A simple missing
generic declaration is therefore not established; investigate full-run
loading/name-resolution context before changing it. Probe evidence is in
`/tmp/scala-rs-jgit-generics-probe/`.

## Rejected candidate 8a336ad4: cross-unit Java type completion

Candidate `8a336ad4`, based on accepted main `f15945b1`, ran the complete
unskipped gate once and reached DONE with VERDICT=FAIL. It is not merged.
Logs: `/tmp/scala-rs-gate-8a336ad4-codex/gate.log`; raw ledger:
`tests/baselines/corpus-8a336ad4.tsv`. The accepted compiler and top-level
metrics remain `5f0d18c2`; the recording commit also records the user's new
batching instruction in `.agent-brief.md`.

The candidate improves gitbucket 191 -> 188 errors (63 files unchanged):
RepositoryResolver and ReceivePackFactory regain type parameters, and
PreReceiveHook regains interface identity. Standard-library errors improve
460 -> 458 (119 files unchanged), removing two AtomicReferenceFieldUpdater
kind errors. Cats is unchanged at 152/58. No diagnostic messages are added.
Slick remains 184 sources, zero errors, 1492 classes; MODE=b passes 12/12
programs and 36/36 attempts, subset verifies 1492 with zero failures and lint
problems, and initialization verification loads all 1492 classes with zero
failures and zero incomplete. Retained class output is under the gate directory.

Workspace: 2704 passed, 1 failed, 297 rows. The failure is
`bparent::binary_parent_prefix_matches_scalac`: the constructors of
bparent.Owner.Empty, reached as RenamedEmpty and bparent.O.EmptyAlias, no
longer resolve. All 5324 corpus identities are unchanged (losses=0,
changes=0); pos 1119/395/345, neg 700/336/369, run 643/864/553. Format passes,
and release workspace clippy retains 57 warnings with no additions.

Evidence confirms the JGit diagnosis with two source files: an earlier
GitServlet subclass introduces shallow member-type declarations, then a
later wildcard import binds RepositoryResolver and ReceivePackFactory without
completing their generic declarations. One source file does not reproduce it.
The permanent jwarm fixture uses a real javac-built library to exercise generic
and nongeneric interfaces in both source orders. Both orders run under
-Xverify:all with byte-identical scalac output; wrong type-argument arity is
rejected independently. The pre-fix binary fails the valid fixture with kind
and mixin errors. Evidence: `/tmp/scala-rs-jgit-generics-probe/`.

Before the gate, 549 related tests passed, the 18 recorded corpus regressions
were unchanged, and preflight validated all four pinned source trees, 121
jars, 33 Java classes and 1498 reference classes. Earlier broad completion
attempts broke string_ops4 by loading JDK String.lines; the final candidate
excludes prelude symbols and that test passes. The full gate exposed the
additional Scala parent-alias boundary. Add bparent to the mandatory focused
suite before the next batch gate. Investigate pickle-first completion rather
than passing every JAVA-flagged Scala declaration through raw classfile loading;
that proposal is a hypothesis and has not yet been tested. The bparent trace
is retained in the probe directory. The gate tree was not edited while running.

An independent bidirectional probe also found unchecked class type bounds:
for `class B[A <: java.lang.Number]`, `B[String]` is falsely accepted in a
parent clause, a value annotation and a type alias; a constructor call is
correctly rejected. Real scalac rejects all four positions. The Java-interface
variant is also rejected by scalac and accepted by the pre-fix compiler with
no preceding unit, establishing a separate root. Evidence is in
`bounds-scope/results.json` and `bound-independent.json` under the probe directory.

Per the user's batching instruction, do not run the next full gate for the
bparent repair alone. Investigate and combine other low/medium-complexity
repairs, including the cats Unit-to-Function0 normalization and Either.type
stability diagnostics, then verify one composed batch. Keep broader type-bound
validation separate if its interactions warrant it.


## Gate forty-one: Java completion, Unit function arity and Either companion

Clean `647afcf2`, containing main `221a2275`, passed one complete unskipped
composed gate with VERDICT=PASS, DONE, corpus losses=0 and changes=1.
Logs: `/tmp/scala-rs-gate-647afcf2-codex/gate.log`. Main was fast-forwarded
to the exact tested commit. The recording commit changes only BASELINE,
the raw corpus ledger and the user's batch-planning instruction in the brief;
compiler sources and tests match the gated tree.

Gitbucket improves 191/63 -> 188/63, cats 152/58 -> 139/57, and the library
460/119 -> 458/119. No diagnostic messages are added in any of these three
source sets. Slick remains 184 sources, zero errors and 1492 classes;
MODE=b passes 12/12 programs and 36/36 attempts. Subset validates all 1492
classes with zero failures and lint problems. Strong initialization verification
with real PostgreSQL and Oracle drivers loads all 1492 classes, zero failures
and zero incomplete. Class output and verify-all.log remain in the gate directory.

Workspace: 2707 passed, zero failed, 298 rows. Format passes. Release workspace
clippy retains 57 warning messages, with no additions or removals. Corpus:
pos 1120/394/345, neg 700/336/369, run 643/864/553 (pass/fail/skip), 5324
unique identities. The sole change is pos/delambdafy-patterns, fail -> pass.
Raw ledger: `tests/baselines/corpus-647afcf2.tsv`.

Three mechanisms were implemented together before the full gate:

- Complete shallow Java type declarations when resolving a type name across
  source units. Prefer Scala pickle adoption before raw Java loading, recovering
  the bparent constructor regression from rejected candidate 8a336ad4. Prelude
  declarations remain excluded. The two-file JGit probe establishes why the
  earlier one-file probe missed the root. The permanent jwarm test builds a
  real Java library, checks both source orders and rejects wrong type arity.
- Preserve the distinction between Unit => A (one argument) and () => A
  (zero arguments) in the parser. The former had been normalized to Function0,
  causing false rejections of valid overrides and false acceptance of a
  zero-argument lambda assigned to Unit => Int. Tests cover aliases, overrides,
  parenthesized Unit, generic calls and nested function results.
- Supply the real Either companion module in library ABI mode. Its missing
  identity caused Either.type stability errors. Tests exercise singleton types,
  Either.cond, module identity and rejection of a Right value as Either.type.

New valid programs execute under -Xverify:all and compare stdout byte-for-byte
with real scalac 2.13.16. Invalid cases compare rejection in both directions;
the rebuilt accepted 5f0d18c2 binary demonstrates each repaired root. Evidence:
`/tmp/scala-rs-jgit-generics-probe/`, including batch-before/results.json.
Before the gate, 694 parser and related CLI tests passed, including bparent;
18 historical corpus regressions had no changes. Preflight validated four
pinned source trees, 121 jars, 33 Java support classes and 1498 reference
classes. One owned execution was followed through DONE with the phase/failure
watcher. No gate step was restarted or skipped.

Read-only probes for the next batch are retained in
`/tmp/scala-rs-next-batch-probes/`. A List[_ <: Base] element incorrectly loses
upper-bound members, including an inherited generic result. A curried implicit
String extension rejects the valid lambda argument and falsely accepts an Int
argument that produces a JVM VerifyError. Real scalac comparisons and runtime
logs establish both defects, but their implementation roots remain hypotheses.
The independent unchecked written type-bound defect from the rejected gate
also remains open. Inventory further candidates and dependencies before the
next implementation batch, rather than limiting it to these first two probes.


## Gate forty-two: seven member/application mechanisms

Clean `3343f368`, based on main `b4431153`, passed one complete unskipped
composed gate with VERDICT=PASS, DONE, corpus losses=0 and changes=0.
Logs: `/tmp/scala-rs-gate-3343f368-codex/gate.log`. Main was fast-forwarded
to the exact tested commit. The recording commit changes only BASELINE and
the raw corpus ledger; compiler sources, fixtures and tests match the gated tree.

Gitbucket improves 188/63 -> 169/61, cats 139/57 -> 125/54, and the library
458/119 -> 444/118. Slick remains 184 sources, zero errors and 1492 classes;
MODE=b passes 12/12 programs and 36/36 attempts. Subset validates all 1492
classes with zero failures and lint problems. Strong initialization verification
with real PostgreSQL and Oracle drivers loads all 1492 classes, zero failures
and zero incomplete. Retained class output and verify-all.log are in the gate
directory. Workspace: 2709 passed, zero failed, 299 result rows. Format passes.
Release workspace clippy retains 57 warnings with no additions or removals.
Corpus: pos 1120/394/345, neg 700/336/369, run 643/864/553 (pass/fail/skip),
all 5324 identities and statuses unchanged. Raw ledger:
`tests/baselines/corpus-3343f368.tsv`.

The user requested a broad inventory and as many practical repairs as possible
before validation. The inventory in docs/batches/member-application.md covers
18 candidate rows, including hypotheses disproved or left unconfirmed by
probes. Seven mechanisms were composed before the single full gate:

- Upper-bounded wildcard receivers look up members through their upper bound,
  including inherited generic members. Runtime checks cover the erased receiver;
  subtype-only and lower-bound-only member accesses remain rejected.
- Fallback to an implicit extension goes through ordinary argument inference and
  remaining-clause handling. The converted receiver substitutes class parameters.
  This repairs both a rejected curried lambda and an accepted Int argument that
  previously emitted a VerifyError, plus generic receiver/result combinations.
- Option.collect, zip and flatten have real polymorphic declarations. zip includes
  both A1 >: A and B; flatten uses actual <:< evidence rather than an erased-shape
  approximation and a fabricated backend witness.
- Try.flatMap, transform and collect retain their independent result parameter;
  orElse, recover and recoverWith retain widening lower bounds. Tests cover
  explicit type arguments, generic methods, exception recovery and lazy fallback.
- Array.toMap carries real <:< evidence and invokes IterableOnceOps through the
  existing ArraySeq wrapping path. Non-pair arrays and mismatched results reject.
- Catch patterns are typed against Throwable, including inferred catch bindings,
  guards and NonFatal extractors, rather than Any.
- An inferred val/def with an unresolved implicit-only method invokes the same
  rejection backstop as an explicitly typed value. It can no longer silently
  become an eta-expanded function. Option[Int].flatten previously compiled and
  threw ClassCastException; the inferred negative and its def variant now reject.

Three valid fixtures execute under -Xverify:all and compare exact stdout with
real scalac 2.13.16. Eighteen negative fixtures independently compare rejection;
no test was removed or weakened. Rebuilt accepted 647afcf2 fails the three valid
fixtures and falsely accepts the invalid flatten, curried-extension and inferred
method probes. Before evidence: `/tmp/scala-rs-next-batch-probes/before-final/`.
The first related suite passes 616 tests; the generic receiver follow-up passes
28 affected tests, and the final released-signature check passes 108 tests.
The 18 recorded corpus regressions have zero changes. Preflight validates four
pinned source trees, 121 jars, 33 Java support classes and 1498 reference classes.
One owned gate execution was watched through DONE; none was restarted or skipped.

Diagnostic comparison removes 21 gitbucket messages and adds two, for a net
reduction of 19. Both Array.toMap missing-member errors are removed. The new
messages are missing toMap evidence in SystemSettingsController: members at
line 395 replaces its old inferred-function mismatch at use on line 400; SQL
row construction at line 300 is newly diagnosed. Do not describe both as merely
relocated diagnostics. The latter exposes an unresolved implicit at an inferred
value boundary; the valid source still needs its inference defect repaired.
Cats and the library add no diagnostic messages. Detailed multisets and location
notes are in diagnostic-comparison.json under the gate directory.

Follow-up evidence in `/tmp/scala-rs-next-batch-probes/` corrects several tempting
next diagnoses. Plain List/Seq/IndexedSeq/Vector/collect toMap, a generic callback,
and JDBC/Java String producers all pass in isolation; the full-run evidence
failure requires tracing loading/inference state, not an assumed missing member
or a blanket String rewrite. Explicitly imported or qualified Manifest with an
explicit Manifest.Int executes identically to scalac, while automatic Manifest
materialization fails. Default alias exposure and materialization are distinct
next candidates. The independent written class-bound validation hole, full-run
collection result issues, higher-kinded implicit evidence and source-class macro
tags remain open. Continue inventory-first batches; this is not compiler completion.


## Rejected evidence-materialization candidate: 9f6995e1

The five-mechanism batch based on accepted main 75f86f78 ran one complete,
unskipped gate and reached VERDICT=FAIL and DONE. It was not merged into main.
The accepted baseline remains 3343f368. Logs are in
`/tmp/scala-rs-gate-9f6995e1-codex/`; raw ledger:
`tests/baselines/corpus-9f6995e1.tsv`.

Gitbucket improves 169/61 -> 158/57 and the library 444/118 -> 440/118, but
cats regresses 125/54 -> 162/58. Slick remains 184 sources, errors=0 and
1492 classes. MODE=b passes 12/12 programs, 36/36 attempts; subset verifies
1492 classes, failed=0 and lint_problems=0. Strong initialization verification
loads all 1492 classes with failures=0 and incomplete=0. Workspace reports
300 rows, 2709 passed and 2 failed: ctoraccessor's scala_library_dual_run_ctacc_fn
and real_scalac_dual_run_ctacc_fn. Format passes; pre-gate release workspace
clippy retains the same 57 warnings.

All 5324 corpus identities compare: pos 1124/390/345, neg 698/338/369,
run 648/859/553 (pass/fail/skip). Changes=15, losses=4: neg/t3507-old,
neg/t5389, run/lift-and-unlift and run/tuples. The other eleven changes
are gains, including recursive and value-class manifests.

The gate exposed three boundaries. A loaded Predef.Function type alias
suppressed lazy term completion of scala.Function, causing the cats regression,
the two workspace failures and two run losses. Source singleton types lose
instance prefixes; checking only the immediate owner still allowed an object
nested under another object inside a class (t3507-old). Default Predef imports
must exclude universal Any/Object members, as nsc's isUnimportableUnlessRenamed
does; otherwise import ne.scala resolves ne through Predef (t5389).
Corrections are developed in a separate worktree while this gate's tree stays
fixed. Their final composed tree requires another complete gate before merge.


## Rejected namespace-integration candidate: 20d4c727

One complete unskipped gate reached VERDICT=FAIL and DONE. Main remains at
accepted compiler 3343f368 (metadata commit 75f86f78). Logs:
`/tmp/scala-rs-gate-20d4c727-codex/`; exact raw ledger:
`tests/baselines/corpus-20d4c727.tsv`.

Gitbucket is 184/60, cats 121/54, and library 442/119 (errors/files). Slick
compiles 184 sources with 9 errors in 2 files and no complete class output.
MODE=b cannot compile; the subset covers only 38 sources / 169 classes,
verified=169, failed=0 and lint_problems=0. This is incomplete Slick coverage;
strong initialization verification of all 1492 classes was not available.
Workspace passes 2711 tests, zero failed, in 300 rows. Format passes and
pre-gate clippy retains the same 57 known warnings.

The full 5324-row corpus has pos 1124/390/345, neg 700/336/369 and
run 647/860/553 (pass/fail/skip). Changes=14, gains=11, losses=3:
run/t10513, run/t3603, run/t6488. The original four candidate losses are
recovered. Java class symbols representing static companions were excluded
from term lookup, breaking wildcard imports. A preceding term-only import
also hid a package class from later type lookup (IntMap in t3603).
The next composed candidate repairs these in a separate worktree and must
pass related regressions and a complete gate before merge.


## Gate forty-three: evidence and default-import batch

Clean 23031519, based on accepted main 75f86f78, passed a complete unskipped
composed gate and reached DONE. The two rejected intermediate gates remain
recorded above with exact raw ledgers. Their frozen worktrees were not edited.
Logs: `/tmp/scala-rs-gate-23031519-codex/`. Main is fast-forwarded to the exact
gated commit; this subsequent record and corpus ledger are metadata only.

Gitbucket improves 169/61 -> 158/57, cats 125/54 -> 121/54, and the library
444/118 -> 440/118 (errors/files). Slick remains 184 sources, errors=0 and
1492 classes. MODE=b passes 12/12 programs and 36/36 attempts. All 1492 classes
verify with lint_problems=0; stronger initialization verification loads all
1492, failures=0 and incomplete=0. Format passes; clippy has no changes to
the 57 known warnings. Workspace and corpus totals above are from this gate.

The five mechanisms isolate inferred val RHS typing inside by-name arguments,
keep explicit TypeApply arguments ahead of implicit application, complete
default Predef aliases lazily, install aliases on their actual declaring module
owner, and materialize recursive Manifest/OptManifest with real library
factories. Full evidence preserves type arguments; unsupported instance paths
still diagnose. Integration preserves distinct term/type lookup, lazy Java
static and Scala binary companion discovery, type lookup after a wildcard
companion use, and nsc's exclusion of universal members from default imports.

New valid fixtures run with the JVM verifier and match real scalac 2.13.16
stdout byte for byte. Invalid fixtures independently compare rejection.
Rebuilt accepted 3343f368 fails the repaired behavior probes; the new namespace
regressions also fail the rejected candidates. The test directory helper uses
an atomic sequence to prevent timestamp collisions without weakening checks.
Final prerequisites pass 558 tests across 14 affected suites, all 60 historical
and newly exposed corpus identities with losses=0, and the full Slick precheck.
Preflight validates four pinned source trees, 121 jars, 33 Java support classes
and 1498 known-good nsc reference classes. Each owned gate was followed through
DONE; no timeout caused a restart and no stage was skipped.

The diagnostic multiset removes 12 gitbucket messages and adds one. SQL evidence
GetResult[Int] at IssuesService:494 replaces three downstream Unit-result
errors; this path remains unresolved. Cats changes two messages at already
failing vector.scala call sites, while ArrayBuilder and Duration change
diagnostics within previously failing library methods. Exact multisets are
in diagnostic-comparison.json under the gate directory.

Next-batch inventory corrects another hypothesis: GetResult[Int] reproduces
without gitbucket. Independent nsc-produced API probes show binary implicit
val/def work; implicit object search fails even after explicit loading/import,
though explicit object selection runs correctly. Further bidirectional probes
find local Array/Function1 classes rejected, while Missing.String, Array type
argument over-arity, and written class-bound violations are falsely accepted.
Both accepted 3343f368 and this candidate share these remaining defects.
Evidence is in `/tmp/scala-rs-evidence-batch-probe/next-implicit-inventory/` and
`next-builtin-inventory/`. Collect and implement these together with related
existing regressions; deeper path-dependent Manifest/TypeTag work stays separate.
Standalone generic Vector scanLeft/scanRight helpers compile and run identically
in all three compilers. The full cats errors require loading/inference context,
not a blanket scan signature replacement; see next-vector-inventory/.
This is measured progress, not a completed Scala compiler.

```text
=== summary
  HEAD=23031519  logs=/tmp/scala-rs-gate-23031519-codex
VERDICT=PASS
DONE
```


## Rejected candidate seventeen: type identity and implicit objects

Clean candidate `4f3f51ef6dfee788dd867c49db42d642f5c1df30` (tree
`1c229916def7255216a48b22809e4c883d3e0241`) ran the complete unskipped gate
and reached DONE. It is not merged. Accepted compiler 23031519 and all current
baseline figures above remain unchanged. Logs: `/tmp/scala-rs-gate-4f3f51ef-codex/`.
Raw 5324-row ledger: [`baselines/corpus-4f3f51ef.tsv`](baselines/corpus-4f3f51ef.tsv).
The candidate branch is pushed and its worktree remains frozen. This record
and ledger are metadata only, not a compiler merge.

Gitbucket regressed 158/57 -> 352/81 and the library 440/118 -> 443/118;
cats remains 121/54 (errors/files). Slick remains errors=0, 1492 classes,
12/12 programs, 36/36 attempts, verified=1492 and lint_problems=0. Stronger
initialization verification loads 1492 with failures=0 and incomplete=0.
Workspace: 301 rows, 2713 passed, 2 failed (cyclic::fixture_cyclic_ok_runs and
gbopt::multi_gbopt_binary_runs). Corpus: pos 1116/398/345, neg 705/331/369,
run 652/855/553 (pass/fail/skip), losses=9 and changes=17. Format passes.

Losses: neg/macro-blackbox-dynamic-materialization, pos/t1001, pos/t1027, pos/t10708, pos/t1292, pos/t2082, pos/t2712-4, pos/t4063, pos/t4970b.

The gate exposed premature written-bound checks for self-recursive parents,
a misowned prelude Using.Releasable type, and implicit candidate result parents
completed after rather than before specificity selection. The negative macro
case exposes loss of source-produced macro bindings when inherited fallback
evidence becomes visible. These are hypotheses under correction in the separate
codex/type-identity-integration worktree; the rejected tree is not edited.
The next composed batch must include both failed existing suites and all nine
corpus identities in its prerequisites. The previous 243-case sample omitted
these nine cases; green focused tests did not establish corpus compatibility.

```text
=== summary
  HEAD=4f3f51ef  logs=/tmp/scala-rs-gate-4f3f51ef-codex
  fail: workspace tests: 301 rows, 2713 passed, 2 failed
  fail: corpus losses=9 vs tests/baselines/corpus-23031519.tsv
VERDICT=FAIL
DONE
```


## Rejected candidate eighteen: macro binding integration

Clean composed commit 2970a6a5972599db4d93f076de13d798d215951e (tree
2cedcb13ccd38157bd6caf4b14e77f9be22470b2) completed the full unskipped gate
with VERDICT=FAIL and DONE. It is not merged. Accepted compiler 23031519 and
the baseline figures above remain unchanged. The candidate branch is pushed
and its worktree remains frozen; corrections use codex/macro-expansion-integration.
Logs: `/tmp/scala-rs-gate-2970a6a5-codex/`. Exact 5324-row ledger:
[`baselines/corpus-2970a6a5.tsv`](baselines/corpus-2970a6a5.tsv).

Gitbucket improves 158/57 -> 157/57 by removing GetResult[Int] at
IssuesService.scala:494, with no added diagnostics. Cats stays 121/54 and the
library 440/118 without changed diagnostics. Slick passes 184 sources, errors=0,
1492 classes, 12/12 programs and 36/36 attempts, verified=1492/lint_problems=0.
Strong initialization verification loads 1492 with zero failures/incomplete
loads. Workspace: 301 rows, 2717 passed, 0 failed. Format passes; clippy keeps
the same 57 warnings.

pos: pass=1118, fail=396, skip=345.
neg: pass=708, fail=328, skip=369.
run: pass=664, fail=843, skip=553.
Full corpus: losses=6, changes=28.

The six pos losses are annotated-original, attachments-typed-another-ident,
attachments-typed-ident, t7461, t8013 and t9392. Source macro bindings now cause
real expansion where earlier classfiles advertised an ordinary nonexistent
method. This exposes block-argument transport, preservation of attachments
through c.typecheck, overloaded-method queries, repeated macro arguments and
ClassDef reconstruction. All nine losses from 4f3f51ef recovered. The green
1405-case negative prerequisite and 322-case pos/run prerequisite missed these
six because their names do not contain macro. Future selection must inspect
source content and numbered compilation rounds, not only test names. Keep
focused dual-run execution and all of these identities before another gate.

Additional bidirectional inventory confirms two existing silent defects in both
rebuilt 23031519 and 2970a6a5: a whitebox implicit macro is skipped for inherited
fallback (99 rather than nsc's 77), and a private binary macro is externally
callable (nsc rejects). Decorated blackbox macros work, correcting the annotation
ordering hypothesis. Standalone generic tuple swap/dimap probes reproduce none
of the two cats tuple errors; both compilers execute identically and reject
the wrong-result counterpart. Evidence is in /tmp/scala-rs-type-identity-batch/
next-macro-inventory/ and next-tuple-inventory/.

```text
=== summary
  HEAD=2970a6a5  logs=/tmp/scala-rs-gate-2970a6a5-codex
  fail: corpus losses=6 vs tests/baselines/corpus-23031519.tsv
VERDICT=FAIL
DONE
```


## Gate forty-four: combined type-identity and macro-transport batch

Clean `73f68974984fde9445a12cdb936a3bf9635df6ab`, tree
`0e0f48b780decf3e575146e3017511dcebce9cc5`, passed the complete unskipped gate and
reached DONE. Logs: `/tmp/scala-rs-gate-73f68974-codex/`. The frozen worktree is
`.worktrees/codex-macro-expansion-integration`. Main fast-forwards to this exact
commit; only this record and the raw corpus ledger are added afterwards. The
previous two rejected intermediate candidates remain recorded, not accepted
retroactively. No full gate was restarted for an observation timeout.

Gitbucket is 158/57 -> 157/57, cats 121/54 and library 440/118 unchanged
(errors/files). The diagnostic multiset removes GetResult[Int] at
IssuesService.scala:494. Repeated argument packing moves the same SQL macro
from an argument-count refusal to an empty-TypeTree refusal; that source
location is still failing. Cats and library diagnostic multisets are identical.
See `diagnostic-comparison.json` in the gate directory.

Slick remains 184 sources, errors=0, 1492 classes, MODE=b 12/12 programs and
36/36 attempts. Subset validates all 1492 with lint_problems=0. The stronger
initialization sweep loads all 1492, failures=0, incomplete=0, using the same
real PostgreSQL/Oracle/reflect dependencies as the accepted baseline.
Workspace: 302 result rows, 2723 passed, zero failed. Format passes. Clippy has
57 existing warnings with no added or removed warnings.

The complete 5324-row corpus has losses=0, changes=38: run gains 27, neg gains
11, pos unchanged. The raw ledger is `tests/baselines/corpus-73f68974.tsv`.
All skip counts remain unchanged. Neg gains include cases accepted before and
now rejected; they are not runtime tests. Runtime gains are actual harness
executions, not compile-only counts.

The composed implementation retains real source type identities and written
bounds, discovers binary implicit objects, exports source macro bindings,
transports structural macro trees/repeated arguments/source positions, preserves
attachments and resets local attributes, repairs qualified Context.Expr tag
materialization, and exports ordinary field storage metadata. Source symbol
identities and lexical owners survive reverse typechecks, including functions,
local vals/defs and local class types. Both changeOwner entry points run Scala's
actual traversal on private synchronized source-symbol adapters; source info is
completed lazily and recursive inferred-owner info still diagnoses. Console
output travels as explicit protocol packets. No phantom macro method, no-op
changeOwner, or dummy owner/type stands in for unsupported behavior.

Before this gate, 852 related tests in 40 suites passed, selected pos/run 452
rows had losses=0, and all 1405 negatives had losses=0. Four pinned source trees,
121 jars, 33 Java support classes and 1498 reference classes passed preflight.
The tested prerequisite binary and gated binary are byte-identical, SHA256
`e85f6f92b424e205e19227a972b81e7a89e8f52e8faec30f466e45831a710cd2`.
`macrotransportbatch` compares both API producers and both consumers with real
scalac 2.13.16, including independent rejections and verifier-enabled exact
stdout comparisons. The original t12576 macro and consumer are rebuilt by
both compilers and execute with identical `List()` output. Accepted230 fails
new valid ownership/transport fixtures and has the separately demonstrated
inferred-implementation VerifyError and missing storage metadata. Detailed
pre-gate investigation is in `docs/batches/macro-transport.md`; its checkpoint
status statements describe the state before this accepted gate.

Next-batch inventory is `/tmp/scala-rs-next-batch-inventory/README.md`. It
prioritizes real Slick SQL interpolation's empty-TypeTree transport, independent
String construction failures (three gitbucket sites reproduce without
Gitbucket), and private-this/bare-constructor storage distinctions. A separate
reflection inspector executes both API outputs: ordinary private/public/lazy/
abstract controls match, while Hidden/Plain/Mutable still expose extra getters.
Accepted230 already has those incorrect getters, so this is a measured remaining
limitation. The deeper explicit dependent macro-signature export issue and
full-run cats Tuple2/NonEmptyList inference remain distinct. These diagnoses
are hypotheses to correct using bidirectional probes, not permission to apply
name-based approximations or to start one gate per repair.

Exact summary block:

```
=== summary
  HEAD=73f68974  logs=/tmp/scala-rs-gate-73f68974-codex
VERDICT=PASS
DONE
```


## Rejected candidate nineteen: SQL and constructor storage batch

Frozen candidate `ee80efb0a3e0341a4551299dd9956faa84e83e21`, tree
`f37200cdd188e46ad0b5615926b92583c92f9de5`, completed the full gate with
VERDICT=FAIL and DONE. It is not merged. Accepted compiler 73f68974 and
all accepted baseline figures remain unchanged. Candidate branch
`codex/sql-constructor-storage-batch` is pushed. Logs are in
`/tmp/scala-rs-gate-ee80efb0-codex/`; the complete 5324-row ledger is
[`baselines/corpus-ee80efb0.tsv`](baselines/corpus-ee80efb0.tsv).

Gitbucket improves 157/57 -> 148/54 (errors/files); cats stays 121/54 and
the library 440/118. Nine gitbucket diagnostic locations disappear, with no
new locations. Slick compiles 184 sources with zero errors and 1504 classes;
subset verification passes all 1504, lint_problems=0. The twelve added named
companions exist in scalac output and were audited before the gate.
However, MODE=b runtime fails all 12 programs and all 36 attempts, with
NoSuchMethodError for synthetic pattern accessors and protected-this pos.
Strong initialization verification passed the identical prerequisite binary's
1504 classes; no separate full-gate strong sweep is claimed.

Workspace: 303 rows, 2721 passed, six failed: catseta eta inference, gbmapto
placeholder diagnostics, outer private-member widening, verify_sql public
user-dollar-outer field, and both verifyfail outer/lib fixtures. Format passes;
clippy retains the 57 existing warnings. Corpus: pos 1126/388/345,
neg 711/325/369, run 677/830/553 (pass/fail/skip), losses=1, changes=4.
Gains are pos/sudoku, pos/t1075 and run/verify-ctor; run/indylambda-boxing
regresses with IllegalAccessError.

The storage hypothesis was too broad: LOCAL includes protected-this, a missing
getter does not imply a private field, and value-class unboxing and widened
private accessors have JVM accessibility requirements beyond source privacy.
The next composed batch corrects these mechanisms alongside value-class
default companions and duplicate macro placeholder reporting. Corrections
are in `codex/sql-storage-access-integration`; the rejected tree stays frozen.
Existing failed suites, Slick execution and the lost corpus identity are
required prerequisites before another full gate. This record and ledger are
metadata only.

```text
=== summary
  HEAD=ee80efb0  logs=/tmp/scala-rs-gate-ee80efb0-codex
  fail: slick_run: progs=12 ok=0 diff=0 fail=12  runs=3 attempts=0/36  (compile-cp=b, work=/private/tmp/claude-501/-Users-shinji-projects-scala-rs/0c32a046-384e-4a5f-9276-add7f58fd709/scratchpad/slickrun/w-0d0a058b60)
  fail: workspace tests: 303 rows, 2721 passed, 6 failed
  fail: corpus losses=1 vs tests/baselines/corpus-73f68974.tsv
VERDICT=FAIL
DONE
```


## Rejected candidate twenty: constructor and value-class access integration

Frozen composed commit `b2502886b656035b4b5532ac19a072f5c8cccef0`, tree
`5840d6d152ec38ca332199a306aac3a8a666f3b0`, completed the entire unskipped
gate with FAIL/DONE. It is not merged. Accepted compiler 73f68974 and all
current baseline figures remain unchanged. Candidate branch
`codex/sql-storage-access-integration` is pushed; its worktree stays frozen.
Logs: `/tmp/scala-rs-gate-b2502886-codex/`. Complete 5324-row ledger:
[`baselines/corpus-b2502886.tsv`](baselines/corpus-b2502886.tsv).

Gitbucket improves 157/57 -> 148/54 with nine removed diagnostic locations
and no added messages. Cats remains121/54 with identical diagnostics; library
440/118 changes only three anonymous-class numeric identifiers. Slick passes
184 sources, zero errors,1504 classes,12/12 programs and36/36 runtime attempts.
Subset verifies1504/lint_problems=0; the stronger initialization sweep loads
all1504 with failures=0 and incomplete=0. Workspace passes2728 tests with
zero failures across303 result rows. Format passes; release workspace clippy
keeps exactly57 existing warning messages with no added or removed warnings.
The prerequisite and gated binaries are byte-identical, SHA256
455a89e285fe18e0ed1d8ed07badb0887a2e8b0fedbf45a5fc39f4b033da29e6.

Corpus: pos1126/388/345, neg711/325/369, run677/830/553 (pass/fail/skip),
losses=1 and changes=4. Gains are pos/sudoku, pos/t1075 and run/verify-ctor.
run/indylambda-boxing from the previous rejection is recovered; the new loss
is run/var-arity-class-symbol, rejected at VarArityClassApi.apply(0).
The matching descriptor/loader diagnosis is still under investigation; the
next prerequisites must include this exact identity and its reflection API
family rather than relying on source keywords. The selected768 pos/run
cases and all1405 negatives passed before this gate, as did639 focused tests
in24 suites and Slick36/36. That selection did not contain the new loss.

The combined corrections repair actual public fields without getters,
protected-this and widened accessors, qualified getter JVM access, value-class
default companions, preservation of Scala parents during classfile completion,
private value-class unboxing getter names and storage/constructor argument
name separation. New fixtures execute all four API producer/consumer pairs
with real scalac2.13.16 and independently compare inaccessible-field rejection.
The previous rejected candidate's six workspace failures are all recovered.
No full gate was restarted for observation timeouts. This subsequent record
and ledger are metadata only, not a compiler merge.

Next-batch inventory under `/tmp/scala-rs-sql-constructor-storage/forms-inventory/`
reproduces the gitbucket webhook mapping failure using real Scalatra Forms.
Explicit ValueType return annotation works and executes identically; inferred
anonymous and named subclasses fail, even when moved before the call. A
dependency-free nested generic argument and repeated type-variable tuple
probe also fail where nsc runs. Independent bad-mapper and wrong-result probes
are rejected by both. Treat nested constraint collection as a hypothesis,
not a license to zip unrelated types or widen everything to Any. Operator-name
export and remaining private/lazy boundaries are separately inventoried.

```text
=== summary
  HEAD=b2502886  logs=/tmp/scala-rs-gate-b2502886-codex
  fail: corpus losses=1 vs tests/baselines/corpus-73f68974.tsv
VERDICT=FAIL
DONE
```


## Gate forty-five: SQL, constructor storage, value-class access and reflection parents

Frozen composed commit `603b6451035c5b35daab28a4783437665756552a`, tree
`8f7afaca580173b406a2b8f08b0c23299708f7a8`, completed the full unskipped
gate with PASS/DONE. Main was fast-forwarded to that exact tested commit.
The subsequent handover commit changes only this record and the raw corpus
ledger; no compiler source, Cargo input, fixture or gate script differs from
the tested tree. The dedicated integration worktree remains frozen and clean.
Logs: `/tmp/scala-rs-gate-603b6451-codex/`. Complete ledger:
[`baselines/corpus-603b6451.tsv`](baselines/corpus-603b6451.tsv).

Gitbucket improves 157 errors / 57 files to **148 / 54**. The diagnostic
multiset removes nine locations with no added errors. Cats stays **121 / 54**
with identical diagnostics. Library stays **440 / 118**, with only three
anonymous-class numeric identifiers changed. These are compile measures;
gitbucket and cats still do not compile successfully.

Slick compiles all **184 sources**, with **zero errors and 1504 classes**.
The twelve added named default-companion classes were audited against real
scalac output before changing the gate's exact class-count expectation.
MODE=b executes **12/12 programs, 36/36 attempts**, with no output differences.
Subset byte verification passes 1504 classes and lint_problems=0. The additional
strong JVM initialization sweep on this gate's actual slick-classes output
reports verify_classes=1504, verify_loaded=1504, verify_failures=0 and
verify_incomplete=0. Its binary is byte-identical to the prerequisite binary,
SHA256 `bd028a67121980c39bea7b2f1a607ad600495154d865266991fdd7da35a66510`.

Workspace: **303 result rows, 2729 passed, zero failed**. Format passes.
Release workspace clippy retains exactly **57** warning messages, with no
additions or removals. Corpus: **pos 1126/388/345, neg 711/325/369,
run 678/829/553** (pass/fail/skip), all 5324 identities present, **losses=0,
changes=3**. Gains are pos/sudoku, pos/t1075 and run/verify-ctor.
Both prior rejected candidates' losses, run/indylambda-boxing and
run/var-arity-class-symbol, are recovered. Their full failed ledgers and
records remain preserved; only this composed PASS tree is integrated.

The batch completes Java constructor members before selection and preserves
java.lang.String's canonical result type. Real Slick sql/sqlu macro arguments
now carry their actual tree types independently of Expr's weak type tag,
including repeated argument elements. Constructor storage, source visibility,
qualified/protected-this/widened accessors, actual public classfile fields,
value-class default companions and unboxing getter identities are handled
together. Macro placeholder reports are deduplicated without hiding errors.
Fixtures execute real parameter binding and all four API producer/consumer
pairs with scalac 2.13.16; independent invalid programs preserve rejection.

The final reflection correction preserves structural FunctionN parents when
converting pickled class parents, including their Scala argument types.
The previous preservation of Scala parent lists exposed that omitted parent;
it was not a simple JVM descriptor mismatch. VarArityClassApi inherits apply
from Function1 and does not declare it itself. The new positive fixture runs
Tuple/Function/Product arity checks under both compilers, while an independent
String-argument probe is rejected by both. The earlier ee80 binary accepted
that invalid String argument; b250 rejected the valid program. This repairs a
silent false acceptance in addition to the recorded corpus regression.

Prerequisites were completed before this full gate: 18 focused tests in four
suites, 595 related tests in twelve suites, all 1405 negative corpus units,
769 selected positive/runtime units including the exact prior losses, and
Slick's full runtime and strong class verification. Environment preflight
validated four pinned source trees, 121 jars, 33 Java support classes and
1498 cached scalac reference classes. The gate was launched once and followed
through DONE with its owned handle; observation timeouts did not restart it.

Inventory for the next composed batch is recorded in
`/tmp/scala-rs-sql-constructor-storage/next-batch-inventory.md` and its
forms-inventory, operator-inventory and cats-warm-inventory subdirectories.
Real Forms inferred-subclass mapping, nested repeated type constraints,
operator/member/type export and escaping, and warmed collection return types
are separately reproduced. The ArraySeq two-file probe reproduces grouped,
sliding and scanLeft/scanRight failures in both source orders; SortedMap.keySet
also fails in one file. These were confirmed with accepted73 and this candidate,
not assumed from the unmerged worktree report. A dotted backtick method is a
pre-existing silent ClassFormatError, confirmed on both binaries. Actual nsc
reflection isolates unencoded ScalaSignature operator names from correctly
encoded JVM names. None of these future repairs is claimed as implemented.
Collect the supported low/medium changes into a batch, retaining especially
interacting inference or collection supply redesign separately when warranted.

```text
=== summary
  HEAD=603b6451  logs=/tmp/scala-rs-gate-603b6451-codex
VERDICT=PASS
DONE
```


## Gate forty-six: nested Forms inference and Scala/JVM name interoperability

Frozen commit `9cc076f6dd0c111036a1c8973cd02869355ad047`, tree
`521048937ab4db109623a8f23808c2b64c1d04a2`, passed the full unskipped gate
and the independent completion audit. Main was fast-forwarded to that exact
commit. The subsequent handover changes only this record and the raw corpus
ledger; no compiler source, Cargo input, fixture or gate script differs from
the tested tree. The dedicated worktree remains frozen and clean.
Logs: `/tmp/scala-rs-gate-9cc076f6-codex/`. Full 5324-row ledger:
[`baselines/corpus-9cc076f6.tsv`](baselines/corpus-9cc076f6.tsv).

Gitbucket improves **148/54 -> 115/54** (errors/files): 33 diagnostic
locations disappear and none are added. Cats remains **121/54**, with identical
diagnostics. Library improves **440/118 -> 439/118**, removing one diagnostic
with no added messages. Gitbucket and cats still do not compile successfully.
Slick remains **184 sources, zero errors, 1504 classes**, with lint_problems=0.
MODE=b executes **12/12 programs, 36/36 attempts**, with exact output matches.
Subset verification passes all 1504 classes. Strong initialization verification
on the full gate's actual slick-classes directory loads **1504/1504**, with
zero failures and zero incomplete loads. MODE=a was not rerun in this gate.

Workspace: **304 result rows, 2735 passed, zero failed**. Format passes;
release workspace clippy keeps exactly the 57 existing warning messages,
without additions or removals. Corpus: **pos 1128/386/345, neg 712/324/369,
run 680/827/553** (pass/fail/skip), all 5324 identities present, **losses=0,
changes=5**. Gains are pos/t7532b, pos/t8708, run/exoticnames, run/t9114 and
the newly correct rejection neg/t1009. The gate and prerequisite binaries
are byte-identical, SHA256
`f92a0f1db75db9a61f9ec978b5adcbf6557fc157ad93da3ffc31f66b4d1d031a`.
The single owned gate ran to DONE in 1321 seconds; no observation timeout
restarted it.

The batch follows a broad prior inventory and combines nested actual-base
alignment for Forms inference, encoded ScalaSignature symbols, NameTransformer
operators and Unicode escapes, backquoted literal escapes, named-parameter
decoding, constant-result types, companion/class alias lookup and nested class
identity/prefix handling. The released Scala NameTransformer oracle covers
all non-surrogate BMP characters with JDK 17. Literal string payloads and
internal storage suffixes retain their raw bytes. ScalaSignature nested class
references retain Outer.this so scalac substitutes the receiving instance.

Bidirectional probes corrected several hypotheses. Real Forms does not depend
on anonymous class identity or declaration order: nested actual-base alignment
is the measured root. Backslash names also needed the lexer's literal escape
handling. Alias results were qualified, not bare nullary members; a companion
without the alias was prematurely ending lookup. Nested class matching split
encoded dollars and invented a different JVM name, independently of the
writer's missing Outer.this prefix. Temporary tracing was removed before
validation. No stubs or subagents were used.

Permanent fixtures execute every valid member/class API through all four
scala-rs/scalac producer-consumer combinations under java -Xverify:all and
compare stdout byte for byte. Independent invalid arguments, aliases, literal
values and malformed identifiers preserve nsc rejection. The immutable
accepted603 binary fails the valid nested-inference/name/alias/literal/nesting
reductions and accepts the independently invalid quoted identifiers. Before
proofs, corrected probe-environment logs and the full inventory are retained
under `/tmp/scala-rs-forms-name/` and the preceding SQL/storage inventory.

Before the full gate, 27 focused cases in four suites and 614 affected existing
tests in 21 suites passed, as did 22 pickle/lexer tests. Historical regression
selection covered 1142 positive/runtime identities with losses=0; all 1405
negative units also had losses=0. Slick runtime and strong verification passed
before freezing the tree. Preflight confirmed four pinned source trees, 121
jar archives, 33 Java support classes and 1498 cached scalac reference classes.
All commands used actual Temurin 17 with a UTF-8 locale; the misleading
Homebrew openjdk@21 symlink was not used.

Next inventory: `/tmp/scala-rs-forms-name/next-inventory.md`. Remaining work
includes ArraySeq-warmed collection return types, SortedMap.keySet and the
deeper repeated-variable constraint solver. Gitbucket's PatchUtil Java source
exists, but the measure script does not compile Java sources: this is a new
source-supply hypothesis to test independently, not a claimed compiler repair.
Any correction of those measurement inputs must be recorded separately from
compiler gains. Continue inventory-first composed batches, isolating only
the especially difficult interacting roots.

Exact summary block:

```text
=== summary
  HEAD=9cc076f6  logs=/tmp/scala-rs-gate-9cc076f6-codex
VERDICT=PASS
DONE
```


## Rejected collection-result candidate a1443e0e (2026-09-11)

Frozen commit `a1443e0eff02df5b2d5c0acf102d82dae85bf12a`, tree
`9ed5ff097789d3cb5ef59031dc206f77970f15df`, was not merged. Accepted
compiler baseline remains **9cc076f6**. This main update records only the
rejected gate and its complete raw ledger:
[`baselines/corpus-a1443e0e.tsv`](baselines/corpus-a1443e0e.tsv).
Logs: `/tmp/scala-rs-gate-a1443e0e-codex/`. The single owned process reached
DONE in 1408.3 seconds. Its shell exit was zero despite VERDICT=FAIL; the
verdict and independent audit are authoritative. No gate was restarted.

The candidate combines receiver-substituted collection declarations, nullary
overload resolution, recovered library value classes, SortedMap.keySet,
evidence-bearing sorted factories and Java String member lookup. It also
restores all 354 gitbucket Scala/Twirl sources and compiles all three real Java
helpers per invocation. Historical-input candidate gitbucket remains **115/54**
(353 sources, one excluded, no Java). Expanded inputs report **108/52**
(354 sources, no exclusion, three Java sources); these are different input
sets and are not presented as a seven-error compiler improvement. Cats reports
**115/48** versus accepted **121/54**, but introduces sorted map diagnostics.
Library remains **439/118**.

Slick: 184 sources, zero errors, 1504 classes; subset verifies all 1504 with
lint_problems=0. MODE=b runs 12/12 programs, 36/36 exact matches. Strong
initialization verification of the actual gate classes loads 1504/1504 with
zero failures or incomplete loads. Workspace has **305 result rows, 2740
passed, three failed**: mismatch10's `mism10_coll_runs_against_the_jar` and
`mism10_sorted_map_collect_after_a_plain_map`, plus typer's
`string_ops4_numeric_range_listbuffer_typecheck_with_library`.

All 5324 corpus identities are present, **losses=0, changes=2**:
pos/t8310 and run/fors gain passes. Counts (pass/fail/skip) are pos
1129/385/345, neg 712/324/369, run 681/826/553. Format passes and clippy
retains the same 57 warnings. The candidate binary SHA256 is
`5354e619961af83805f66729dcca2d6692de70359454ea460d41339d35f9555f`.

Before the gate, 684 tests in 32 suites, 1288 selected pos/run identities and
all 1405 negatives passed their checks; source/jar/cache preflight was intact.
The selection omitted mismatch10 and typer's embedded legacy String test.
Follow-up selection must search fixture contents as well as test names, and
include typer unit tests. A real two-source TreeMap.map probe independently
confirms a regression: scalac and the immutable accepted binary compile and
execute both input orders, while this candidate rejects after Map/ArraySeq
warming. Generic SortedMap.map also has a pre-existing warm-only failure.
The legacy lines.next() assertion is false on actual JDK17 scalac; its valid
positive form is linesIterator.next(), with the old form kept as a rejection
probe. No expectation is weakened to preserve an invalid program.

Follow-up work is isolated in codex/collection-results-followup. It combines
preserving additional overload completion, per-alternative origin validation,
late ClassTag factory edges and materialized ClassTag viability, with the
corrected legacy test. Do not run another full gate for only one repair.
The failed gate worktree stays frozen; main compiler sources remain unchanged.

Exact summary block:

```text
=== summary
  HEAD=a1443e0e  logs=/tmp/scala-rs-gate-a1443e0e-codex
  fail: workspace tests: 305 rows, 2740 passed, 3 failed
VERDICT=FAIL
DONE
```


## Gate forty-seven: collection results, evidence factories and Java members

Frozen commit `913fb1c0cc3a9e42ad62deb0408dc7a609e4bc69`, tree
`f2a578b8e106aa499457ab1a9ea73d37f26b068f`, passed the unskipped full gate and independent
completion audit. Main was fast-forwarded to that exact commit. The handover
then changes only BASELINE.md and the raw corpus ledger; compiler sources,
Cargo inputs, fixtures and scripts match the tested tree. The dedicated
worktree remains frozen and clean. Logs: `/tmp/scala-rs-gate-913fb1c0-codex/`.
Raw ledger: [`baselines/corpus-913fb1c0.tsv`](baselines/corpus-913fb1c0.tsv).
The single owned gate reached DONE in 1337.9 seconds, exit zero.

Cats improves **121/54 -> 103/45**, with 18 diagnostic locations removed and
none added after normalizing generated anonymous IDs. Library remains
**439/118**, with its entire diagnostic log byte-identical. Gitbucket under
historical inputs remains **115/54**; expanded coverage is **108/52** across
354 Scala/Twirl sources and three actual Java sources compiled with javac.
The aggregate input correction is separate from compiler gains. Gitbucket
and cats still do not compile successfully.

Slick remains 184 sources, zero errors and 1504 classes. All 12 runtime
programs pass, all 36/36 attempts match scalac output. Subset verifies 1504
classes with lint_problems=0. Independent strong JVM initialization on this
gate's actual slick-classes loads 1504/1504, failures=0, incomplete=0.
MODE=a and the separate specialization ledger were not rerun or claimed green.
Workspace has 305 result rows, 2745 passed and zero failed. Format passes.
Clippy retains exactly the 57 accepted warning identities, with none added.
All 5324 corpus identities are present; losses=0, changes=3:
pos/implicits-old, pos/t8310, run/fors. Counts are recorded above.

The batch follows the broad inventory in docs/batches/collection-results.md:
receiver-substituted collection results, covariant SortedMap keys, library
value classes, duplicate nullary overloads, six sorted evidence factories,
lazy ArraySeq/ClassTag factories, result-constrained implicit clauses, real
Java String precedence, and complete gitbucket inputs. SortedMap/TreeMap
additional Ordering overloads survive earlier generic Map loading. Each
supplied alternative is checked separately rather than discarding the entire
set. No stubs or subagents were used.

The first composed candidate a1443e0e was rejected and preserved in the preceding record.
Its follow-up combines the measured overload regressions with the complete
ArraySeq factory family. The old positive lines.next() is rejected by actual
JDK17 scalac; both its embedded unit source and fixture now use linesIterator,
with identical expected output, and the old expression remains a negative
oracle. A shared Java test directory allocator now uses an atomic counter
and exclusive creation; the observed missing-class failure alone did not prove
a timestamp collision. The gate emits DONE on both verdicts and now returns
nonzero on FAIL; both summary control paths were tested.

Before freezing, 1426 tests in 121 affected CLI suites and 190 typer unit tests
passed, including the prior mismatch10 regressions. Every new valid program
runs real scalac and scala-rs under java -Xverify:all with byte-identical stdout,
in multiple source orders where relevant. Independent invalid programs reject;
immutable accepted-before probes fail the repaired behaviors. Broad oracle
results, corrected hypotheses and evidence live under
/tmp/scala-rs-collection-results and /tmp/scala-rs-collection-followup.

The broad prerequisites used e28fed905f738bd2. After removing a redundant
function-final return to clear a new clippy warning, the final binary reran
21 focused tests, two protected-access tests five times, 1292 selected pos/run
units and all 1405 negatives (zero losses), and cats (byte-identical diagnostics).
Preflight reconfirmed four pinned source trees, 121 jar archives, 33 Java support
classes byte-for-byte and 1498 cached reference classes. Actual Temurin17 and
UTF-8 were used throughout. The final prerequisite and full-gate binary hashes
match: `b2409aadc8f1b45b3ac32989c08aedc622c2d6b305022af77036aa11af68d761`.

Next inventory: /tmp/scala-rs-collection-followup/next-inventory.md. It contains
13 families to reproduce, including generic builders, anonymous implicit
members, deferred functions, Java wildcard SAMs, Slick option operations and
exception overloads. Deeper constraint solving and source-type macro reflection
remain separately scoped hypotheses. No new defect is inferred solely from an
error message; silent runtime and bidirectional rejection probes remain required.

Exact summary block:

```text
=== summary
  HEAD=913fb1c0  logs=/tmp/scala-rs-gate-913fb1c0-codex
VERDICT=PASS
DONE
```


## Rejected dependent adaptation candidate: `5ddf4fc9`

Frozen commit `5ddf4fc97097a58cb117e946cfa33fe9dda238ae`, tree `f0b49018abe4515be49e07fbcbcdc1c8890b57f6`,
completed the unskipped full gate with VERDICT=FAIL and DONE in
1343.6 seconds. It is not approved for main; accepted compile
numbers and the baseline ledger above remain at 913fb1c0.
Logs: `/tmp/scala-rs-gate-5ddf4fc9-codex/`; raw ledger:
[`baselines/corpus-5ddf4fc9.tsv`](baselines/corpus-5ddf4fc9.tsv).

The workspace run has 306 result rows, 2752 passed and 1 failed.
The sole failure is batchtypes::either_companion_matches_scalac, at creation of
its root directory: AlreadyExists / File exists at batchtypes.rs:13. This is
before case compilation. A census finds timestamp-only naming without an
independent nonce in 40 CLI test files; the follow-up adds a shared monotonic
process-specific stamp and deterministic same-time/concurrent/reversed-clock
checks. Compiler sources and Scala fixtures are unchanged in that follow-up.

All 5324 corpus identities are present, losses=0, changes=20.
Counts: {"neg": {"fail": 323, "pass": 713, "skip": 369}, "pos": {"fail": 367, "pass": 1147, "skip": 345}, "run": {"fail": 824, "pass": 683, "skip": 553}}.
Slick remains zero errors, 1504 classes, all 12 programs and 36/36 byte-exact
runtime attempts. Subset class validation and lint pass. Independent strong
JVM verification loads and initializes all 1504 classes, failures=0,
incomplete=0. Cats is 90/37, gitbucket 105/49 on the unchanged full 354-source
input plus three actual Java files, and library 421/114. These are candidate
measurements, not accepted baseline numbers. Compared with 913fb1c0, diagnostic
locations remove 13 cats, three gitbucket and 18 library errors, with no new
locations; three existing library locations have changed type wording.

The seven-mechanism batch was checked against real scalac2.13.16 and exact
runtime output. Broad prerequisites passed 1567 CLI tests and 198 typer tests.
Selected corpus prerequisites caught four regressions before the full gate:
t8310, t3619, saito and t1623. They were repaired together with added independent
runtime and rejection controls; final prerequisites pass 520 CLI tests,
198 typer tests, 1390 selected pos/run units (zero losses, 14 gains), all
1405 negatives (zero losses, one gain), format, unchanged 57 clippy warnings,
and the source/jar/cache preflight. All intermediate prerequisite ledgers are
retained under /tmp/scala-rs-dependent-adaptation. No subagents or stubs.

Exact summary block:

```text
=== summary
  HEAD=5ddf4fc9  logs=/tmp/scala-rs-gate-5ddf4fc9-codex
  fail: workspace tests: 306 rows, 2752 passed, 1 failed
VERDICT=FAIL
DONE
```


## Gate forty-eight: dependent results, SAMs, implicit overrides and self types

Frozen commit `b969b0d1beaf5e8ed29a0ca5fb005e77848af30a`, tree
`5ca65c05011129163f06f5db6e8d7ded72883b32`, passed the unskipped full gate, reached DONE in
1343.5 seconds, and passed the independent completion audit. Main was
fast-forwarded to this exact tested commit. The handover then changes only
BASELINE.md and the raw corpus ledger. No compiler source, Cargo input,
Scala fixture, test harness or script changed after the gate. The dedicated
worktree remains frozen and clean. Logs: `/tmp/scala-rs-gate-b969b0d1-codex/`; raw ledger:
[`baselines/corpus-b969b0d1.tsv`](baselines/corpus-b969b0d1.tsv).

On unchanged input sources, cats improves 103/45 -> 90/37, gitbucket
108/52 -> 105/49, and library 439/118 -> 421/114 (script-reported file counts;
see the multiline diagnostic counting note above). No new diagnostic location
appears: 13 cats, three gitbucket and 18 library locations disappear. Three
existing library locations change their type wording. Complete diagnostic logs
are byte-identical to the first candidate 5ddf4fc9. Gitbucket and cats still do
not compile successfully; this is measured progress, not compiler completion.

Slick has 184 sources, zero errors and 1504 emitted classes. All 12 runtime
programs and 36/36 attempts match scalac byte-for-byte. Subset validates all
1504 classes with lint_problems=0. Strong actual JVM loading/initialization on
this gate's own slick-classes reports 1504 loaded, zero failed and zero
incomplete. MODE=a and the separate specialization ledger were not rerun and
are not claimed green. Workspace: 307 result rows,
2755 passed, zero failed. Format passes; clippy has exactly
57 existing warning occurrences with none added or removed.

The corpus contains all 5324 unique identities, with zero losses and
20 gains. Exact statuses and counts are above; raw diagnostics
are retained in the ledger. Changes: neg/sammy_expected, pos/context, pos/depmet_1_pos, pos/sammy_exist, pos/sammy_scope, pos/scoping1, pos/scoping3, pos/t0039, pos/t10418_bounds, pos/t1049, pos/t1050, pos/t10792, pos/t11558, pos/t3371, pos/t360, pos/t361, pos/t372, pos/t3861, run/t6443, run/try-catch-unify.

The batch follows the inventory in docs/batches/dependent-adaptation.md and
implements seven mechanisms together: immutable.Seq erasure of pickled repeated
parameters; Java SAM completion, ground targets and contravariant inference;
actual argument singletons in dependent results; ordinary overrides hiding
inherited implicits in the correct lexical/constructor scope; declared self
requirements checked separately from instantiation; polymorphic apply through
self/singletons without repeated owner substitution; and nested function result
prototypes that preserve the inferred type. Real scalac probes corrected the
initial FunctionN and import-routing hypotheses, and exposed a silent
Consumer[AnyRef]/String-lambda ClassCastException in the before compiler.

There are 35 independent oracle programs in eight groups, covering valid
execution with java -Xverify:all and exact stdout bytes, rejection in both
compilers, source warming order, primitive boxing, lazy execution and receiver
evaluation count. Eighteen programs expose the accepted-before defects. Initial
prerequisites passed 1567 CLI tests in 142 suites and 198 typer tests. Selected
corpus testing then exposed t8310/t3619, and full negatives exposed saito/t1623;
all four were fixed with permanent independent controls before the full gate.
Final compiler prerequisites passed 520 tests in 21 CLI suites, 198 typer
tests, 1390 pos/run units (zero losses, 14 gains), all 1405 negatives (zero
losses, one gain), clippy, format and preflight. No aggregate before run,
stubs, or subagents were used.

The first full candidate 5ddf4fc9 failed on a clock-only temporary directory
collision before case compilation. Its complete rejected gate and ledger are
preserved in the preceding record and were pushed at 7377993d. The follow-up
659d4dd8 fixes all 40 CLI test files identified without an independent nonce,
using shared monotonic process-specific stamps and deterministic concurrent,
same-time and backwards-clock tests. All 169 tests in the 41 affected suites
passed; the 35 compiler oracle programs were rerun in that worktree. Compiler
sources and Scala fixtures remain byte-identical to 5ddf4fc9. The latest main
record was then merged automatically to form the exact b969b0d1 gate tree.

The final gate binary matches the harness prerequisite binary:
`cf35c34a0c24c0c27481720074c7a41ada3c923f135cdce6193867826a39234f`. It is a separate worktree build from
5ddf4fc9's 48f19da0ff44d9d4 binary, with identical compiler sources. Preflight
reconfirmed four pinned source trees, 121 jars, 33 Java support classes
byte-for-byte and 1498 reference cache classes. Actual Temurin17 and UTF-8
were used throughout. Evidence: /tmp/scala-rs-dependent-adaptation/harness.

Next inventory: /tmp/scala-rs-dependent-adaptation/next-inventory.md groups
13 probe families with hypotheses and execution/rejection controls.
/tmp/scala-rs-dependent-adaptation/remaining-diagnostics.json contains every remaining diagnostic and its source window, including multiline
messages. Generic parent constructor inference, generic inner-class chains,
Slick option/join/macro reflection and upper-wildcard argument capture remain
separate measured or explicitly unresolved roots. Error wording alone is not
a diagnosis; bidirectional and executable probes remain the prerequisite.

Exact summary block:

```text
=== summary
  HEAD=b969b0d1  logs=/tmp/scala-rs-gate-b969b0d1-codex
VERDICT=PASS
DONE
```


## Rejected contextual inference candidate: `ef3551c6`

Frozen commit `ef3551c6e21a46db83bf393c8766814de71649bf`, tree `4228007b033e04b46472d945e966cf33af632004`, based on local main
`56bf357f1a9164edf6dff63422b7b33cb86fb678`, completed the unskipped full gate in 1534.8
seconds, with script VERDICT=FAIL, process exit=1 and DONE.
Independent acceptance audit is FAIL: gitbucket regresses from 105 to 2675
errors. This candidate is not merged. The accepted compiler, metrics and
baseline ledger above remain at b969b0d1. The main handover contains only this
rejection record and the raw candidate corpus ledger; it is not the gated
compiler tree. The frozen compiler is retained on
`codex/contextual-inference-batch`.

Logs and audits: `/tmp/scala-rs-gate-ef3551c6-codex/`. Raw candidate ledger:
[`baselines/corpus-ef3551c6.tsv`](baselines/corpus-ef3551c6.tsv).
The script verdict does not enforce the gitbucket error budget; it is not
sufficient for acceptance. Independent audit problems: ["gate verdict is not PASS", "gate process failed", "gitbucket input or error regression", "new diagnostic locations", "corpus losses"].

Candidate measurements use the unchanged pinned inputs: cats 339 sources,
one skip, 83/34 errors/reported files (was 90/37); gitbucket 354 Scala/Twirl
sources plus three real Java files, no skips, 2675/186 (was 105/49); standard
library 538 sources, 421/114 (unchanged). Complete diagnostic comparison finds
seven cats removals and no additions, 2572 gitbucket additions and two removals,
and no library additions/removals. Most new errors are ambiguous Twirl
`_display_` calls. Reported file counts retain the measurement scripts' grep
convention; full multiline locations are retained in diagnostic-audit.json.

Workspace: 2762 passed, 0 failed,
308 result rows. Corpus: 5324 identities,
losses=1, changes=6; counts:
{"neg": {"fail": 321, "pass": 715, "skip": 369}, "pos": {"fail": 367, "pass": 1147, "skip": 345}, "run": {"fail": 822, "pass": 685, "skip": 553}}.
The sole loss is pos/val_infer: an inferred val implementing an Int-returning
member has a String initializer and a valid implicit String-to-Int conversion.
The new override result check rejects this case. It was outside the selected
pos/run prerequisites and is added to the next combined repair inventory.
Gains are neg/i10715b, neg/t473, pos/t927, run/t6928-run and run/t7436.
Slick: errors=0, 1504 classes, 12 programs, 36/36 byte-exact runtime attempts,
subset validation and lint clean. Independent JVM verification loads and
initializes all 1504 classes: failures=0, incomplete=0. The prerequisite
clippy comparison retains 57 warning occurrences, with none added or removed.

This was a single full gate after a combined implementation of inferred
implicit overrides, residual-clause overload handling, implicit method eta
expansion and receiver capture, parent repeated arguments and JVM packing,
erased override identity, pickle variance, materialized-tag inference and
module self references in parent arguments. No subagents or stubs.
The permanent oracle matrix has 33 Scala programs in seven groups, with real
scalac 2.13.16 bidirectional acceptance and byte-exact verified JVM execution;
19 expose a defect in the immutable before executable. Initial prerequisites
passed 1585 CLI tests in 146 suites and 198 typer tests. Selected corpus and
full negatives found four intermediate regressions, repaired together before
the full gate. Final parent prerequisites passed 527 CLI tests in 22 suites,
198 typer tests, 1418 selected pos/run units and all 1405 negatives, with no
losses; format, clippy and source/jar/cache preflight also passed. Evidence:
`/tmp/scala-rs-contextual-inference/parent-prerequisites/`.
The frozen executable SHA-256 is
`4cf69f47affaabbaa6d05af309683c7255339450d818a12ddc7bb83e375da4da`.

Read-only diagnosis while the gate ran reproduced the regression against the
real twirl-api_2.13 2.0.9 jar. A subclass of BaseScalaTemplate[Html, Format[Html]]
calls `_display_` with Seq[Any], null explicitly typed Any, and Int. Both real
scalac and the accepted before executable compile and execute it with output
`&lt;x&gt;1`, an empty line, and `42`; this candidate rejects all three calls as
ambiguous. Untyped null is correctly ambiguous in scalac too, so it is kept as
a negative control, not claimed as a regression. Runtime reflection of the
actual jar distinguishes `(AnyVal)T` from `(Any)(implicit ClassTag[T])T`, which
share an Object argument after JVM erasure. A source-defined AnyVal/Any pair
also fails on the accepted before executable, exposing an additional existing
defect. The initial hypothesis that both first clauses were Any was disproved.
Evidence and executable probe scripts:
`/tmp/scala-rs-contextual-inference/twirl-regression/`.
The next implementation batch must cover both source and binary overload
origins, retain valid/invalid controls and include other ready repairs before
another full gate. No edits or rebuilds were made to the frozen tree during
this gate, and no one-fix full-gate retry was started.

Additional next-batch inventory reduced gitbucket's named curried constructor
failure to executable case-class programs, with and without defaults in the
two argument lists. Both are accepted and executed by scalac and rejected by
the accepted before compiler; a wrong-type control is rejected by both. A
case companion explicitly inheriting Function2 uses tupled correctly in both
compilers, so that reduction does not justify a tupled repair. Evidence:
`/tmp/scala-rs-contextual-inference/next-constructor-probes/`.

Exact summary block:

```text
=== summary
  HEAD=ef3551c6  logs=/tmp/scala-rs-gate-ef3551c6-codex
  fail: corpus losses=1 vs tests/baselines/corpus-b969b0d1.tsv
VERDICT=FAIL
DONE
```
