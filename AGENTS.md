# AGENTS.md

scala-rs is a Rust implementation of a Scala 2.13 compiler that emits JVM
class files. The reference behavior is scalac 2.13.16.

## Layout

`crates/`: `span`, `lexer`, `parser`, `pickle` (ScalaSignature reader),
`typer`, `backend` (bytecode and pickle emission), `driver`, `cli`.
Integration tests live in `crates/cli/tests/` and are registered through
`crates/cli/tests/shards/shard_NN.rs`.

## Build and test

```sh
cargo build --release -p scala-rs-cli
cargo test --release --workspace --no-fail-fast
```

- Tests expect scalac at `/tmp/scala-2.13.16/bin/scalac` and the library at
  `/tmp/scala-rs-lib/scala-library-2.13.16.jar`. Restore them before running
  the suite; a missing toolchain produces many spurious failures.
- Use JDK 17. Some test data depends on the JDK version.
- `tests/BASELINE.md` records the accepted measurements, and
  `tests/verify_merge.sh` is the merge gate. See `docs/testing.md`.

## Rules

- Write all documentation, comments and commit messages in English.
- A fix needs a regression test that fails without it. Prefer tests that
  compare with scalac: compilation results, program output, or a scalac
  client compiled against our class files.
- Judge regressions against a run of the same suite on the base commit, not
  against an assumed green state.
- Fix root causes, and match the comment density and style of the
  surrounding code.
- `.agent-brief.md` holds detailed lessons for longer tasks.
