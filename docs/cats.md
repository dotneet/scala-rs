# typelevel/cats

Where this compiler stands on [typelevel/cats](https://github.com/typelevel/cats),
the second real-world benchmark after slick. It is a library written in the
type-class style: higher-kinded type parameters, implicit syntax layers,
kind-projector type lambdas, and one macro.

## The material

| | |
|---|---|
| Repository | `https://github.com/typelevel/cats` |
| Revision | **`32a50dcfad9d897459bb755c4b5a22b4c7bc745c`** (tag `v2.13.0`) |
| Modules | `kernel` and `core` (`cats-kernel`, `cats-core`) |
| Scala | 2.13.16 |
| Sources | 340 (95 kernel, 245 core), of which 16 are generated |

`v2.13.0` is pinned rather than `main` because its published jars
(`cats-kernel_2.13-2.13.0.jar`, `cats-core_2.13-2.13.0.jar`) are in the local
Coursier cache, so a single file can be compiled in isolation against the rest
of cats when a symptom needs narrowing down. The revision and the expected
source counts are recorded in `tests/fixture_manifest.toml`.

`kernel` and `core` are the whole dependency chain: `core` depends on `kernel`
and nothing else (`algebra` depends on `kernel` too, but `core` does not depend
on `algebra`). `laws`, `free`, `alleycats` and `tests` are further out.

### What sbt actually compiles

Read off `sbt "show coreJVM/Compile/unmanagedSourceDirectories"` and
`"show coreJVM/Compile/scalacOptions"`, not guessed:

* Source directories for 2.13 are `scala`, `scala-2` and `scala-2.13+`;
  `scala-2.12` and `scala-3` are not compiled.
* 16 sources are **generated** by sbt source generators
  (`project/KernelBoiler.scala`, `project/Boilerplate.scala`): 1 for kernel and
  15 for core. Measuring without them would ask for a source set real scalac
  never sees, so `tests/cats_measure.sh` has sbt write them once.
* Dependencies are `scala-library`, `scala-reflect` (Provided, for the one
  macro) and `scalac-compat-annotation_2.13`.
* **`-Xsource:3` is on** (sbt-typelevel's default; only the `algebra`
  subproject opts out). Same as slick.
* **Two compiler plugins are on: kind-projector 0.13.3 and
  better-monadic-for 0.3.1.** better-monadic-for only changes how a `for`
  comprehension is desugared. kind-projector's syntax is handled as below.

### `-Ykind-projector`

`-Ykind-projector` is **not an nsc flag**. kind-projector is a compiler
plugin; nsc without it rejects `Either[E, *]` and `λ[α => F[α]]` exactly as
this compiler does with the flag off, and that rejection is correct, so it
stays the default. With the flag, a syntactic pass over the parsed type trees
(`crates/parser/src/parse/kindproj.rs`) rewrites the plugin's syntax into
structural type lambdas, following the trees
`scalac -Xplugin:kind-projector… -Xprint:kind-projector` prints.
`crates/cli/tests/kindproj.rs` pins both directions.

### `-no-specialization`

The measure also passes nsc's `-no-specialization`, because cats annotates with
`@sp`; see
[scala-corpus.md](scala-corpus.md#why-pos-does-not-pass--no-specialization)
for why that flag is right for a type-checking measure and wrong for the
corpus.

## Measuring and running it

```
CATS_LOG=<your own path> CATS_RUN=<your own path> tests/cats_measure.sh
```

It rebuilds *this* checkout's `target/release/scala-rs` (or uses
`SCALA_RS=<binary>`), re-fetches the material at the pinned revision when it
is missing, and writes every path per invocation. `CATS_LOG` and `CATS_RUN`
default to per-invocation paths under `SCALA_RS_FIXTURE_ROOT`; set them when
you want to keep the output. The script passes
`-Xsource:3 -no-specialization -Ykind-projector` itself and prints one line:
`files= skipped= errors= files_with_errors= classes= compiler_exit=`.

* `CATS_MODULES=kernel` measures kernel alone; `CATS_MODULES=core` measures
  core alone against the published `cats-kernel` jar; the default is both from
  source.
* `CATS_EXCLUDE` holds files out. It defaults to empty.

A compile measure says nothing about whether the output works.
`tests/cats_run.sh [prog-name ...]` is the differential execution test: it
compiles cats with scala-rs and with real scalac, compiles the client programs
in `tests/catsrun/` with real scalac against **both** builds, runs them under
`java -Xverify:all`, and compares stdout byte for byte. A client compiled
against scalac's cats that behaves differently when run on our classes shows a
**codegen** defect; a client that fails when compiled against our cats shows a
**pickle** defect, since real scalac then reads our `ScalaSignature`s and runs
our macro classfiles. See
its header for the environment variables. The merge gate runs both scripts.

## Where we stand

Per `tests/BASELINE.md` (gate `ff08907d`, 2026-09-14), `tests/cats_measure.sh`
compiles all **340 files with 0 errors and emits 2977 class files**, and
`tests/cats_run.sh` runs **9/9** client programs with `known_fail=0`. The
merge gate requires zero errors and no run failure outside the (empty)
known-failure ledger.

## Known gaps

The main sources compile and the client programs run; cats' own test suites
are the next layer. The last recorded measurement, in
[notes/handoff-2026-09-13.md](notes/handoff-2026-09-13.md) (2026-09-14, not
merge-gated), ran cats' 155 munit suites with cats main compiled by scala-rs
and the support/test layers compiled by real scalac:

* 13,835 tests ran, 613 failed, in 62 failing suites. The dominant roots were
  unnecessary outer capture (335 serialization failures), erased value-class
  identity (238 `ClassCastException`s), 30 stack-safety failures and four
  `AbstractMethodError`s.
* scala-rs itself does not yet compile the support layers: `kernel-laws`
  (36 errors), `laws` (a compiler stack overflow) and `testkit` (27 errors).

## History

Earlier versions of this page recorded the whole path from 755 parse errors to
zero: the symptom breakdowns, the kind-projector measurement, and one section
per slice (`agent/kernel`, `agent/catstail`, `agent/catstail3`,
`agent/catseta`, `agent/projection`, `agent/hkunify`, `agent/convimpl`,
`agent/sortedmap`, `agent/basetypeargs`, `agent/catsrest`, `agent/catszero`
and others), each with its reductions. Code comments and tests that cite
those sections (for example "`docs/cats.md`'s `Newtype` note" or "the
`Type::TypeMember` has no prefix note") refer to that record; read it with
`git log -p -- docs/cats.md`.
