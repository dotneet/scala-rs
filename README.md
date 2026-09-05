# scala-rs

A compiler for a subset of Scala 2.13 (nsc), written in Rust. It reads Scala
sources and emits JVM class files.

It is not a port of scalac's sources. It is an original reimplementation.
Scala 3 syntax and TASTy are out of scope.

## Status

Experimental. This is a subset compiler, not a finished Scala compiler, and it
makes no claim of conformance to the language specification. What exists today:

- The front end carries an AST close to nsc's `Tree`: namer, typer (including
  implicit search), uncurry, lambda-lift and erasure.
- The target is Java 8 class files (major version 52), with a `StackMapTable`
  (`full_frame`) in the `Code` attribute. Frame types for locals are the erasure
  of the slot's declared type, as in scalac.
- Two ABI modes. By default the compiler links against a real
  `scala-library-2.13.x` jar when it can find one; with `--no-scala-library` it
  emits its own private runtime class files (`scala/Option`, `scala/List`,
  `scala/FunctionN`, …) instead. See [Library modes](#library-modes).
- Lambdas are emitted as `invokedynamic` through `LambdaMetafactory`, like nsc
  2.13. `PartialFunction` literals and arities above 22 are still compiled to
  anonymous classes.

For what the language subset does and does not cover, see
[docs/language-support.md](docs/language-support.md) and
[docs/not-implemented.md](docs/not-implemented.md).

## Benchmark

The reference workload is [slick](https://github.com/slick/slick): 184 files,
23,337 lines, compiled with `-Xsource:3` against scala-library 2.13.16 plus
slick's dependency jars. Both compilers were measured on the same machine under
the same conditions, back to back.

|                | wall    | CPU     | class files |
| -------------- | ------- | ------- | ----------- |
| scalac 2.13.16 | 12.0 s  | 68.6 s  | 1498        |
| scala-rs       | 1.8 s   | 1.7 s   | 2127        |

Medians of three runs, alternating between the two compilers so that both see
the same machine. The CPU-time gap is the larger one: scalac's wall time is
carried by several threads, while scala-rs runs the compile itself on one
thread and only parallelises writing the class files.

scala-rs emits more class files than scalac for the same sources, so the
comparison is not entirely in its favour: `PartialFunction` literals still
become anonymous classes here.

What that run establishes:

- All 184 slick files typecheck, with 0 errors.
- All 1552 emitted class files load under `java -Xverify:all`. They *load*:
  `Class.forName(initialize = false)` links nothing, so method bodies are not
  verified by that number (see `tests/slick_run.sh`).
- The test suite is 130 test binaries / 1849 tests. 84 of them (the programs in
  `tests/conform/`) are dual-run against real scalac 2.13.16 and required to
  produce byte-identical stdout.

This is one benchmark, not a completeness claim. A large real program compiling
and verifying says nothing about the parts of the specification it happens not
to use.

Methodology, phase breakdown and profiling notes are in
[docs/performance.md](docs/performance.md).

## Build

A Cargo workspace. The CLI crate is `scala-rs-cli`; the binary is `scala-rs`.

```bash
cargo build -p scala-rs-cli --release
```

Or run it straight from the workspace:

```bash
cargo run -p scala-rs-cli -- compile file.scala -d out/
```

The binary lands in `target/release/scala-rs` (or `target/debug/scala-rs`).

## Usage

Compile sources into a directory of class files:

```bash
scala-rs compile file.scala -d out/
scala-rs compile file.scala -d out/ --scala-library /path/to/scala-library-2.13.16.jar
scala-rs compile file.scala -d out/ --no-scala-library
scala-rs compile B.scala -d outB -cp outA --no-scala-library
scala-rs compile file.scala -d out/ -Xsource:3
```

Compile and run the entry point (`main` in an `object Main`). `run` adds the
library jar to `java -cp` when one is in use:

```bash
scala-rs run file.scala
scala-rs run file.scala --scala-library /path/to/scala-library-2.13.16.jar
scala-rs run file.scala -- arg1 arg2
```

The emitted class files are launched by `java` exactly as scalac's are: an
`object` produces a module class `Main$` plus a forwarder `Main` carrying the
static `main`.

```bash
java -cp out Main
java -cp out:scala-library-2.13.16.jar Main
```

### Library modes

`--scala-library [<jar>]` (or the `SCALA_LIBRARY_JAR` environment variable)
links against the **scala-library 2.13 ABI**: `Option`, `List`, `FunctionN`,
`Tuple2`, `Predef$`, the `Rich*` / `StringOps` / `ArrayOps` extension methods,
the collections, `Either`, `scala.util.Try`, `scala.jdk.CollectionConverters`,
and so on come from the jar, and no colliding private class file is emitted.
Members that are not in the hand-written prelude are supplied on demand by
reading the `ScalaSignature` pickle out of the jar's class files. If the path is
omitted, `SCALA_LIBRARY_JAR`, `/tmp/scala-rs-lib` and the current directory are
searched.

`compile` and `run` use an auto-detected jar by default and fall back to the
private runtime when there is none. `--no-scala-library` forces the private
runtime.

### Debug and diagnostic flags

- `--parse` — parse only and dump the AST (no typechecking, no output).
- `--typer` — dump the tree after namer/typer. This is a dump flag, not a stop
  flag: the compile still runs to the end.
- `-Xfatal-warnings` — turn warnings (non-exhaustive match, …) into errors.
- `-Xsource:<version>` — source level: `2.13` (default), `3` or `3-cross`. The
  `3` levels accept the Scala 3 spellings this subset implements (`A & B`
  intersection types). As in nsc, a level below the current major is an error.
- `-language:<feat>` — enable `postfixOps`, `implicitConversions` or `dynamics`.
- `-cp` / `--class-path` — read previously compiled class files, Scala classes
  in jars (through their `ScalaSignature` pickle) and Java `.class` files from
  jars, jmods and the JDK.
- `SCALA_RS_PICKLE_DEBUG=1` — trace which library members were supplied from a
  pickle, and why the others were not.

`scala-rs --help` prints the full list.

## Project layout

| crate                | role                                                                    |
| -------------------- | ----------------------------------------------------------------------- |
| `crates/span`        | source positions and diagnostics                                        |
| `crates/lexer`       | lexing (newline tokens for semicolon inference, interpolation modes)     |
| `crates/parser`      | recursive-descent parser; AST close to nsc's `Tree`                     |
| `crates/pickle`      | reader for nsc `ScalaSignature` pickles, shared by typer and backend     |
| `crates/typer`       | namer, typer, implicit search, uncurry, lambda-lift, erasure             |
| `crates/backend`     | JVM class file emission (major 52, `StackMapTable`) and the private runtime |
| `crates/driver`      | pipeline driver                                                         |
| `crates/cli`         | command line; binary `scala-rs`                                         |

Test data lives outside the crates: `tests/fixtures/` (single-file programs plus
their expected stdout), `tests/multi/` and `tests/conform_multi/` (multi-file
programs), `tests/conform/` (the differential conformance corpus).

## Testing

```bash
cargo test
```

Tests that need external artifacts skip themselves rather than fail when the
artifact is missing:

- Anything comparing against real scalac needs `scalac` 2.13.16 on `PATH`.
- Anything in library mode needs `scala-library-2.13.16.jar`, found through
  `SCALA_LIBRARY_JAR` or `/tmp/scala-rs-lib`.
- The Java-side checks need `java` (and, for a few of them, `jar` and `javap`).

The scripts under `tests/` are measurement harnesses, not part of `cargo test`:

- `tests/bench.sh` — time a full compile of slick's 184 sources and report wall
  and CPU time. `--parse` times parsing only; `REPS=n` repeats.
- `tests/slick_measure.sh` — compile slick's sources and report the error count
  (correctness, not speed). It rebuilds its own toolchain and re-clones slick at
  a pinned revision when pieces are missing.
- `tests/slick_subset.sh` — find the fixpoint of slick files that compile
  cleanly together, emit their class files, and load every one of them with the
  bytecode verifier on.
- `tests/slick_run.sh` — build slick twice (scala-rs and real scalac), compile
  the client programs in `tests/slick_progs/` once with real scalac, and run
  that one client binary against each slick build, comparing stdout byte for
  byte. The first harness that asks whether the emitted slick *runs*. Each
  program is executed `RUNS` times (default 3) and the per-program `m/n` is
  printed, so an intermittent failure cannot be averaged away. See
  [docs/notes/running-the-slick-we-compiled.md](docs/notes/running-the-slick-we-compiled.md).
- `tests/expand_fm.py` — expand the seven FreeMarker templates slick's build
  generates Scala sources from, so a measurement covers what sbt would compile.
- `tests/testkit_measure.sh` — the same measurement for `slick-testkit`, slick's
  own test suite, compiled against the class files scala-rs produced for slick.
- `tests/reap_strays.sh` — kill `scala-rs` processes orphaned by a killed test
  run (`--kill`; without it, only reports).

How the fixtures, the dual-run harnesses and the pickle-reader regression tests
are organised is described in [docs/testing.md](docs/testing.md).

## Documentation

- [docs/language-support.md](docs/language-support.md) — the implemented
  language subset, feature by feature.
- [docs/not-implemented.md](docs/not-implemented.md) — what is knowingly
  missing.
- [docs/architecture.md](docs/architecture.md) — crate structure, and how
  library symbols are supplied from `ScalaSignature` pickles.
- [docs/performance.md](docs/performance.md) — benchmark methodology, phase
  breakdown, and the optimisations behind the numbers above.
- [docs/testing.md](docs/testing.md) — test layout and what each suite fixes.
- [docs/comparison-with-scalac.md](docs/comparison-with-scalac.md) — an honest
  diff against scalac 2.13.
- [docs/macros.md](docs/macros.md) — design notes for def macros, and
  [docs/macro-engine-prototype/](docs/macro-engine-prototype/) — the
  feasibility probe behind them (not production code).
- [docs/slick-testkit.md](docs/slick-testkit.md) — compiling slick's own test
  suite: what is measured, the numbers, and what they found.
- [docs/cats.md](docs/cats.md) — where this compiler stands on typelevel/cats,
  the second real-world benchmark.
- [docs/scala-library.md](docs/scala-library.md) — compiling scala/scala's own
  `src/library` from source: the numbers, and the one root behind them.
- [docs/specialization.md](docs/specialization.md) — `@specialized`: the
  annotation is accepted and recorded, the phase that emits `Foo$mcI$sp` is
  not. `tests/spec_classfiles.sh` measures the gap against real scalac.
- [docs/notes/](docs/notes/README.md) — development notes: the investigations
  and the reasoning behind individual changes.

## License

Apache-2.0
