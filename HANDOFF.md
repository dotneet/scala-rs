# scala-rs — session handoff

**Codex continuation in progress:** read
[`docs/notes/codex-validation-2026-09-06.md`](docs/notes/codex-validation-2026-09-06.md)
for recovered branch checkpoints, newly reproduced defects, Java environment
requirements, and unfinished validation. The original three pending slices
and their required fixes were independently validated at `d9eb5dc` and merged
locally as `4b0568af`. Read the updated `tests/BASELINE.md`, including its
per-test corpus reference and red MODE=a check. The historical results below
describe the previous session, not the current baseline. This continuation
has not pushed its commits.

Written 2026-09-06 at the end of a long parallel-development session. Its
purpose is to let the next session pick up without re-deriving anything.

Everything measured here was measured by the coordinator, on the merged tree,
not taken from a slice's report. Where a slice's report and a measurement
disagreed, the measurement is what is written.

## Where things stand

`main` is pushed to `https://github.com/dotneet/scala-rs.git`. The numbers that
gate every merge live in [`tests/BASELINE.md`](tests/BASELINE.md) — **read that
file rather than measuring the baseline yourself.**

| | at the start of this session | now |
|---|---:|---:|
| cats (kernel + core, 339 files) | 1108 errors / 139 files | **474 / 88** |
| gitbucket (353 files) | 1373 / 184 | **912 / 111** |
| scala library (`src/library`, 538) | 1903 / 172 | **1647 / 171** |
| corpus `pos` (1859) | 980 | **1053** |
| corpus `run` (2060) | 444 | **585** |
| corpus `neg` (1405) | 656 | **659** |
| slick (184 files) | `errors=0`, 1596 classes | `errors=0`, **1490** (nsc emits 1498) |
| slick execution | 12/12 | **12/12**, `attempts=36/36` |
| `cargo test --workspace --release` | 153 binaries / 1990 | **190 / 2193**, 0 failed |
| `tests/verify_all.sh` (slick, 1490 classes) | 6 failures (undetected) | **0** |

42 slices merged, 330 files changed. Compile time is 1.47 s against nsc's 12 s.

## The two things that matter more than the numbers

**1. Trait ABI compatibility.** Traits used to compile to `T$class` static
helpers; nsc 2.13 compiles them to interface default methods. A subclass
compiled by nsc **could not find our trait implementations at all** — a hole
under the word "compatible" that no error count showed. `crates/cli/tests/traitclass.rs`
now checks the real thing: scala-rs compiles the trait, real scalac compiles a
subclass, they run together, and the output matches scalac compiling both.
Writing that one test exposed seven further defects, including `$init$` missing
from the pickle so that **every `val` in one of our traits was silently null**.

**2. What each check actually proves.** Several checks were reporting green
over real defects. Both of these were found this session and both are now
closed:

* `slick_subset.sh` calls `Class.forName(name, **false**, loader)`. Not
  initialising means not linking, and an unlinked class never has its method
  bodies verified. **Six of slick's 1490 classes had been failing JVM
  verification for an unknown number of waves.** `tests/verify_all.sh` now
  loads every class with `true`, and all six are fixed — the check reports 0.
* Neither `javap` nor that call reads the `Signature` attribute at all, so 23
  classes throwing `MalformedParameterizedTypeException` were invisible until a
  slice asked `java.lang.reflect` for the generic form of all 26273 members.

And the sharper version, learned while fixing the six: **a verifier's failure
count is a lower bound on miscompilation.** Fixing the root behind those six
turned up four more classes broken the same way that verify fine — an
`invokespecial` to the current class is legal bytecode, it is just infinite
recursion, and the verifier has no opinion about that.

## Open work, in the order I would take it

### Running when this session stopped

Three slices had committed work on branches and were still verifying. **Their
work is preserved; pick them up rather than restarting.**

| branch | worktree | subject | commits |
|---|---|---|---:|
| `agent/implicitmemo` | `.claude/worktrees/agent-a4d7c4b87749307cb` | memoise implicit search on `(wanted type, depth)` | 6 (+2 files uncommitted) |
| `worktree-agent-a44905be6f76c9f6a` | `.claude/worktrees/agent-a44905be6f76c9f6a` | `Null` erasure + cross-unit `IllegalAccessError` | 7 |
| `agent/catstail3` | `.claude/worktrees/agent-a83e60577131de73f` | cats' remaining flat tail | 9 |

All three had finished implementing and were partway through their final
battery when the session stopped. `agent/catstail3` had already measured the
full corpus and matched main exactly (pos 1053, neg 659, run 585), so its
remaining work is the workspace test and the compile measures.

For each: `git merge main` in its worktree, run `cargo test --workspace
--release`, the measures the change can reach, and the full corpus; compare
against `tests/BASELINE.md`; merge if nothing is lost.

### 1. Implicit search memoisation — a prerequisite, already measured

`agent/gbimplicit` found that gitbucket's largest remaining family (~170
`value list / update / firstOption is not a member of Query[…]`) has a
**one-line fix that works**, and did not merge it, because it takes gitbucket's
213 hand-written files from **14 seconds to over 13 minutes**. 5591 samples all
sit on one stack, `search_implicit_at → implicit_fit_at → search_implicit_at`,
with no memo and a depth limit of 8. The diagnosis, the repro and the timing
are in [`docs/gitbucket.md`](docs/gitbucket.md). `agent/implicitmemo` was
started on exactly this.

### 2. `@specialized` stage 2

Stage 1 landed: the annotation is accepted and recorded, and 78 tests went
green. Stage 2 is the phase itself — `Foo$mcI$sp` classes and the ABI that goes
with them. `tests/spec_classfiles.sh` is the ledger and says **RED**: of the 37
`pos/spec-*` tests, 2 match scalac's emitted class names and scalac emits 700
`$sp` classes to our 0. Five `neg` tests are owed to this phase too. Design in
[`docs/specialization.md`](docs/specialization.md).

### 3. Tail-call optimisation

`@tailrec` is now accepted wherever nsc accepts it and **nothing optimises the
call** — `crates/backend/` has no tail-call transform. A `final` method
recursing two million times overflows here and returns in nsc. Nothing
diagnoses it and the corpus cannot catch it (a test that proves the point takes
a stack overflow to fail). See [`docs/not-implemented.md`](docs/not-implemented.md).

### 4. `mapTo`'s 31 gitbucket errors — needs a design decision first

`agent/macrotag` lifted the "cannot pass a class from the current run to a
macro" limit for `TableQuery` (35 errors) by sending a placeholder symbol.
`mapTo` cannot use that, because `mapToImpl` does not *carry* the type argument,
it **interrogates** it — `isCaseClass`, `companion.info.member("tupled")`, each
accessor's `typeSignature` — and while `lazy val X = TableQuery[X]` is being
typed we cannot answer truthfully. That needs a reverse RPC from the macro
engine back into the typer, which is the same open problem `c.typecheck` has.
`docs/macros.md` §7.18 has the analysis. **Half of it is worse than none of
it**: `mapToImpl` would build a tree from a class it only half understands.

### 5. The rest of corpus `run` (585 / 2060)

Reclassified each wave, and the composition keeps being the interesting part.
Of the 121 output-mismatch/assert failures examined most recently: **32 cannot
go green whatever the compiler does**, because partest builds a `run` log as
compiler output followed by program output and `tests/scala_corpus.sh` compares
only stdout/stderr. Fixing that is two steps — teach the harness to prepend the
compile log, *then* implement the warnings — and neither is useful alone.

`reify` is a dead end for now, measured twice: `agent/reify` moved 0 of 163 and
`agent/reifydefs` moved 0 of 108. 147 of the 163 depend on a toolbox, which is
why `agent/toolbox` then moved 38 of 128. Do not widen reify further without a
new reason.

### 6. Smaller, all with repros in the docs

* `Null` erases to `Object` where nsc uses `scala.runtime.Null$` — an ABI
  difference (`agent/nullcross` was on this).
* A value class over a *reference* underlying type throws `ClassCastException`
  at the call site.
* An extractor's sub-pattern may name a type the field cannot hold; scalac says
  `scrutinee is incompatible with pattern type`.
* `scala.StringContext.s` is declared in the prelude and does not exist in
  2.13.16 — a hand-written `sc.s(…)` links to nothing.
* `identifierCase`: a title-case initial (`ǅ`, U+01C5) is read as a variable
  pattern, so the first arm silently matches everything.
* 546 prelude/pickle fidelity differences are inventoried in
  [`docs/prelude-fidelity.md`](docs/prelude-fidelity.md); the ones that make us
  *accept too much* (SEALED missing on 15 classes, ABSTRACT on 10, FINAL
  spurious on 128) are deliberately still open.

## How to run slices

[`.agent-brief.md`](.agent-brief.md) is the permanent brief every slice reads.
It has grown by about a dozen sections this session, all of them written after
something went wrong. The ones that cost the most:

* **Launch every slice with `isolation: "worktree"`.** Five once landed in the
  shared checkout at the same time and one moved the branch out from under the
  others.
* **Read `tests/BASELINE.md`; do not measure the baseline.** Six slices used to
  spend twenty minutes each establishing figures the previous slice had already
  established.
* **Rebuild before any manual `scala-rs` invocation.** The parent tree's binary
  goes stale and produces a *plausible* wrong answer. It fooled the coordinator
  into "correcting" a merged slice that was right.
* **`git stash` is shared across worktrees.** Two slices' stashes collided.
* **A count is not a yield.** An error count falls by roughly what the root
  carried; a corpus *test* count is an upper bound and often a very loose one
  (≈200 → 3, 163 → 0, 128 → 38).
* **Verify code motion with byte-identical output.** Splitting `check.rs`
  changed behaviour once, silently: a private `fn` had been shadowing a `pub`
  one arriving through a glob import, and moving its caller selected the other
  with no error and no warning.

## What I would not trust without re-measuring

* Any diagnosis in `docs/` that was reasoned out rather than measured. One
  carried across several waves — "a pickled declaration cannot be told from a
  definition", complete with a measured 1693 → 2117 warning — was refuted by
  one `javap -p`. The measurement was real; it measured a fix for a problem
  that did not exist.
* My own cluster estimates. They were wrong in both directions repeatedly:
  579 → duplication, 707 → wrong cause, 220 → 1, 288 → 28, ≈259 → 217 with a
  different diagnostic word, 247 → 123 across two roots, 93 → 140 for a reason
  the brief had backwards.
