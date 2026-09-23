# scala-rs

scala-rs is an experimental Rust implementation of a Scala 2.13 compiler. It
reads Scala source files and emits Java 8 JVM class files. The project is an
independent reimplementation; it does not contain scalac's source code.

Scala 3 syntax and TASTy are outside the project's scope.

## Status

The compiler targets a measured subset of Scala 2.13, with scalac 2.13.16 as
the reference. It is exercised against real code bases, and the class files
it emits are run and compared with scalac's output:

- **Cats**, **GitBucket** and **Slick** compile without errors, and programs
  built from the emitted classes produce the same output as scalac's.
- The **Scala standard library** sources (`src/library`) type-check apart from
  a few unimplemented features.
- **refined** `coreJVM` compiles from unmodified sources; its 528 tests,
  compiled by scalac against the generated jar, all pass.
- A differential corpus built from the scala/scala test suite
  (`pos`, `neg` and `run`) tracks acceptance, diagnostics and runtime output.

These projects are also where the remaining compatibility gaps are found, so
scala-rs is not a drop-in replacement for scalac. Current figures are
recorded in [`tests/BASELINE.md`](tests/BASELINE.md).

The compiler supports two runtime modes:

- `--scala-library <jar>` (the default when a Scala 2.13 library is found)
  links generated classes against the real library.
- `--no-scala-library` emits the project's private runtime classes.

See [the language support guide](docs/language-support.md) and
[known gaps](docs/not-implemented.md) for the current boundary.

## Building

Requirements:

- Rust stable with Cargo
- JDK 8 or newer (the test suite is pinned to JDK 17)
- Scala 2.13.16 for differential and interoperability checks

Build the CLI in release mode:

```sh
cargo build --release -p scala-rs-cli
```

Compile a source file:

```sh
target/release/scala-rs compile Main.scala \
  --scala-library /path/to/scala-library-2.13.16.jar \
  -d out
```

Run a source file directly:

```sh
target/release/scala-rs run Main.scala \
  --scala-library /path/to/scala-library-2.13.16.jar
```

Use `scala-rs --help` for all compiler options, including `-cp`,
`-Xfatal-warnings`, `-Xsource:3` and `-Ykind-projector`.

### Macros and async

- Scala 2 def macros from the classpath are expanded; see
  [macros](docs/macros.md).
- `async` / `await` from scala-async 1.0.1 work with `-Xasync` and the
  scala-async jar on the classpath. They are rewritten into non-blocking
  `Future` callbacks, including non-local `return` and
  `markForAsyncTransform` for custom libraries. See [async](docs/async.md).

## Testing

Run the workspace tests with:

```sh
cargo test --workspace --release
```

The tests expect scalac at `/tmp/scala-2.13.16/bin/scalac` and the library at
`/tmp/scala-rs-lib/scala-library-2.13.16.jar`. They cover parsing, typing,
bytecode generation, runtime behavior, separate compilation in both
directions, Java interoperation and comparisons with scalac. The long-running
compatibility gates and the merge gate (`tests/verify_merge.sh`) are
described in [the testing guide](docs/testing.md).

## Documentation

- [Architecture](docs/architecture.md): compiler crates and compilation phases
- [Language support](docs/language-support.md): implemented Scala features
- [Known gaps](docs/not-implemented.md): intentionally incomplete behavior
- [Testing](docs/testing.md): test layout and validation commands
- [Comparison with scalac](docs/comparison-with-scalac.md): compatibility scope
- [Performance](docs/performance.md): benchmark methodology
- [Scala library](docs/scala-library.md): standard-library source results
- [Cats](docs/cats.md), [GitBucket](docs/gitbucket.md) and
  [Slick testkit](docs/slick-testkit.md): real-world targets
- [refined](docs/refined.md): refined `coreJVM` build verification
- [Macros](docs/macros.md), [async](docs/async.md),
  [specialization](docs/specialization.md) and [tail calls](docs/tailrec.md)
- [Development notes](docs/notes/README.md): focused implementation records
- [Batch records](docs/batches/): measured changes and follow-up inventories

## Contributing

Small, reproducible changes are easiest to review. Include a regression test,
the command used to validate it, and any comparison with Scala 2.13.16 that
supports the change. Read the relevant design note before changing a compiler
phase; many compatibility fixes depend on interactions between the typer and
backend. Guidance for coding agents is in [AGENTS.md](AGENTS.md).

## License

Apache-2.0
