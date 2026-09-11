# scala-rs

scala-rs is an experimental Rust implementation of a Scala 2.13 compiler. It
reads Scala source files and emits Java 8 JVM class files. The project is an
independent reimplementation; it does not contain scalac's source code.

Scala 3 syntax and TASTy are outside the project's scope.

## Status

The compiler targets a useful, measured subset of Scala 2.13. It is exercised
against the Scala standard library, Cats, GitBucket, Slick, and a differential
test corpus using Scala 2.13.16 as the reference compiler. These projects are
also where the remaining compatibility gaps are found, so this is not a
drop-in replacement for scalac.

The compiler supports two runtime modes:

- `--scala-library <jar>` (the default when a Scala 2.13 library is available)
  links generated classes against the real library.
- `--no-scala-library` emits the project's private runtime classes.

See [the language support guide](docs/language-support.md) and
[known gaps](docs/not-implemented.md) for the current boundary.

## Building

Requirements:

- Rust stable with Cargo
- JDK 8 or newer
- Scala 2.13.16 when running differential or interoperability checks

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

Use `scala-rs --help` for all compiler options.

## Testing

Run the workspace tests with:

```sh
cargo test --workspace
```

The test suites cover parsing, typing, bytecode generation, runtime behavior,
separate compilation, Java interoperation, and comparisons with scalac. The
long-running compatibility gates are documented in
[the testing guide](docs/testing.md); their accepted measurements are recorded
in [`tests/BASELINE.md`](tests/BASELINE.md).

## Documentation

- [Architecture](docs/architecture.md) — compiler crates and compilation phases
- [Language support](docs/language-support.md) — implemented Scala features
- [Known gaps](docs/not-implemented.md) — intentionally incomplete behavior
- [Testing](docs/testing.md) — test layout and validation commands
- [Comparison with scalac](docs/comparison-with-scalac.md) — compatibility scope
- [Performance](docs/performance.md) — benchmark methodology
- [Scala library](docs/scala-library.md) — standard-library source results
- [Cats](docs/cats.md) and [GitBucket](docs/gitbucket.md) — real-world targets
- [Slick testkit](docs/slick-testkit.md) — separate compilation and runtime checks
- [Development notes](docs/notes/README.md) — focused implementation records
- [Batch records](docs/batches/) — measured changes and follow-up inventories

The repository keeps detailed design notes and validation evidence in `docs/`
so that this page can remain a quick project overview.

## Contributing

Small, reproducible changes are easiest to review. Include a regression test,
the command used to validate it, and any comparison with Scala 2.13.16 that
supports the change. Please read the relevant design note before changing a
compiler phase; many compatibility fixes depend on interactions between the
typer and backend.

## License

Apache-2.0
