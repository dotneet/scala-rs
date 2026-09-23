# slick-testkit

`tests/slick_measure.sh` compiles slick's own 184 sources. `slick-testkit` is
the layer above: slick's test suite, which is a *user* of slick's API and
reaches it through the class files the compiler emitted rather than through
types the typer just computed from source. It therefore measures separate
compilation in both directions: whether scala-rs can read slick's class files,
and whether the class files scala-rs writes carry what a reader needs.

## What is measured

`tests/testkit_measure.sh [main|test|all|both] [extra scala-rs args...]`.

Stage `main` is testkit's compile scope: `slick-testkit/src/main` (48 files)
plus the four modules `build.sbt` puts on its `compile` configuration --
`slick-codegen` (5), `slick-future` (2), `slick-hikaricp` (1) and `slick-zio`
(0 main sources) -- **56 files**. Stage `test` is `slick-testkit/src/test`
(33 files, minus `GeneratedCodeTest.scala`, whose sources sbt generates by
running the code generator against a live H2) and is compiled on top of stage
`main`'s output, so it is blocked until stage `main` compiles. `all` runs the
two stages as two compilations. Stage `both` compiles the two together in one
invocation, which is the only way to get a figure for `src/test` while stage
`main` still fails.

The classpath is slick's compiled classes (pass the directory
`slick_measure.sh` left behind as `SLICK_CLASSES=<dir>`; the script compiles
slick itself when it is not given one), slick's `deps.cp`, and junit /
HikariCP / ZIO, which the script fetches with `cs fetch` on first use.
`TESTKIT_DIR` and `TESTKIT_LOG` choose the work directory and log.

`TESTKIT_SLICK_SRC=1` puts slick's *sources* into the same compilation instead
of its class files. Same program, different supply path: a diagnostic that
appears only in the class-file configuration is a class-file emit or read bug,
not a testkit one.

Two ways to tell the reader from the writer:

* compile testkit against **slick 3.6.1 from Maven**: class files scalac wrote
  are known good, so every diagnostic is scala-rs's reader or typer;
* have **real scalac** compile against the class files scala-rs wrote: every
  diagnostic is then something our `ScalaSignature` lost.
  `crates/cli/tests/testkit2.rs` does this with
  `tests/fixtures/testkit2_{lib,use}.scala`, in both directions, and
  `crates/cli/tests/testkit.rs` holds the rest of this layer's regressions.

## Where we stand

The testkit is not part of the merge gate, and `tests/BASELINE.md` does not
track it. The slick main compile it builds on is at 184 files, 0 errors,
1504 classes there, with `tests/slick_run.sh` at 12/12 programs.

The last recorded measurement of slick's test sources, in
[notes/handoff-2026-09-13.md](notes/handoff-2026-09-13.md) (2026-09-14, not
merge-gated), used a 39-source layered harness that is not in this
repository: **357 errors in 28 files**, of which 57 in the two
`CodeGenRoundTrip*` sources are harness artefacts (their generated roundtrip
sources are absent). The largest real groups were `column` (23),
`CanBeQueryCondition` (20), `O` (15), `Tracer` implicits (13) and Future/DBIO
overloads (10). Running the suite under junit waits on those sources
compiling.

## Known gaps

* `tests/testkit_measure.sh` still looks for the slick checkout and its
  `deps.cp` under a fixed, session-specific scratchpad path, while
  `tests/slick_measure.sh` now keeps them under `SCALA_RS_FIXTURE_ROOT`. Its
  "run `tests/slick_measure.sh` once first" hint therefore does not make it
  work; the script needs to use the fixture helpers before its numbers can be
  reproduced.
* The compile-scope errors listed above.

## History

Earlier versions of this page recorded three slices on this layer: the
`import tdb.profile.api.*` fix that took `not found` from 1478 to 52, the four
reader roots found against the Maven jar (`agent/testkit2`), and the writer
fix that declares nested classes in the enclosing pickle and splits
oversized signatures into `ScalaLongSignature` (a third slice), each with
before/after numbers and the gaps it left. Code comments and tests that cite
those sections refer to that record; read it with
`git log -p -- docs/slick-testkit.md`.
