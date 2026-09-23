## Testing

```bash
cargo test --workspace --release
```

### Layout and running

The CLI integration suite keeps its fixture sources under `crates/cli/tests/`,
but Cargo links them through eight shard targets to avoid hundreds of separate
test binaries. Run one or more fixture modules by source name with:

```bash
tests/cli_test.sh vcbridge
tests/cli_test.sh conversion_inference implicitmemo bparent
```

The helper resolves each module to its shard and applies a module-qualified
test filter, so it does not execute unrelated tests in that shard. Direct
commands such as `cargo test --test vcbridge` are no longer valid Cargo target
names; use `tests/cli_test.sh` for focused work and
`tests/workspace_tests.sh` for the parallel full workspace suite.

`crates/cli/Cargo.toml` sets `autotests = false`, so Cargo discovers nothing
in `crates/cli/tests/` by itself. The only targets are `cli_shard_01` …
`cli_shard_08`, whose sources are `crates/cli/tests/shards/shard_NN.rs`. A new
test file `crates/cli/tests/<name>.rs` must be registered in exactly one shard
with a `#[path = "../<name>.rs"] mod <name>;` pair (the shards are kept in
alphabetical order), or it is never compiled. `tests/cli_test.sh` refuses a
module that is registered in no shard or in more than one. Give a new area of
work its own test file rather than appending to a shared one such as `e2e.rs`,
so that parallel branches do not conflict.

`tests/workspace_tests.sh` builds the workspace tests once with `--no-run`,
then runs the test binaries in parallel (`WT_JOBS` binaries × `WT_THREADS`
test threads, 6 × 4 by default) and ends with
`workspace_tests: binaries=N rows=M missing=K failed_bins=F doc_rows=D`.
`missing` counts binaries that died without printing a result. It applies an
1800-second wall-clock limit to each test binary. A timeout kills the binary's
process group (including compiler/JVM children), records a deterministic
`workspace_tests: TIMEOUT` marker, and is counted as both a missing result and
a non-zero binary. Set `WT_TIMEOUT=0` only for an intentional, unbounded run;
malformed values fail before the build. `tests/run_with_timeout_test.sh`
smoke-tests the guard without building the workspace. `.cargo/config.toml`
caps the interactive `RUST_TEST_THREADS` at 6 so a `cargo test` leaves the
machine usable.

### Compare against the base commit

The full workspace suite is not guaranteed to be green on `main`: on
2026-09-23 a full run on `main` had 47 failing tests that predate the work
being tested. Before attributing a failure to a change, run the same suite
(or the same `tests/cli_test.sh` modules) on the base commit and compare the
failing test *names*, rather than assuming the base passes everything. The
same holds for every other check: record the base's result next to yours.

### Toolchain and JDK

Tests are pinned to **JDK 17** (Temurin 17.0.3 on the reference machine).
Checked-in data depends on it: `crates/pickle/src/java_identifier_parts.rs`
and `crates/lexer/src/unicode_symbols.rs` are generated with JDK 17, and
`tests/fixtures/expected/numt.txt` records JDK 17's `Float` / `Double`
`toString` spelling. On a newer JDK,
`names_match_released_scala_for_bmp_and_escape_sequences` and the `numtower`
tests fail without any compiler change. Check `java -version` before
debugging those, and never regenerate these files with another JDK.

The external fixture scripts take the Scala 2.13.16 home, library, compiler
jars, and `scalac` launcher from `[toolchain]` in
`tests/fixture_manifest.toml`. Relative paths use `/tmp` by default (the
existing fixture layout); set `SCALA_RS_TOOLCHAIN_ROOT` to relocate them, or
use `SCALA_HOME`, `SCALA_LIBRARY_JAR`, `SCALA_LAUNCHER_LIBRARY_JAR`,
`SCALA_COMPILER_JAR`, `SCALA_REFLECT_JAR`, or `SCALAC` to override the
corresponding Scala path, or `JAVA`/`JAVAC` for explicit JDK tools. The latter
take precedence, followed by the corresponding `JAVA_HOME/bin` tools, then
`PATH`; the same selected `java` is recorded in cache fingerprints and
embedded in the generated `scalac` launcher.

With the defaults that means `/tmp/scala-2.13.16` (the `scalac` launcher and
the compiler, reflect and launcher-library jars) and
`/tmp/scala-rs-lib/scala-library-2.13.16.jar`. Both are wiped by a reboot.
`tests/slick_measure.sh` rebuilds them from the Coursier cache when they are
missing, and `crates/cli/tests/e2e.rs` downloads the Scala 2.13.16
distribution into `/tmp` when no `scalac` is found — but only when that test
runs, so tests that run earlier fail with missing-file errors. Restore the
toolchain *before* a full suite run (running `tests/slick_measure.sh` once is
enough), and check `ls /tmp/scala-2.13.16/bin/scalac` first when many
scalac-comparison tests fail at once. Project checkouts and caches live under
`SCALA_RS_FIXTURE_ROOT` (default `${TMPDIR:-/tmp}/scala-rs-fixtures`), and
their revisions and expected source counts are pinned in
`tests/fixture_manifest.toml`.

### Fixtures

Runtime expectations live in `tests/fixtures/`. For each `.scala` there is a
`.txt` of the same name in `tests/fixtures/expected/` (stdout, with the
trailing newline `println` produces), and as far as possible that file is
real scalac 2.13.16's stdout verbatim. A fixture is exercised in up to three
modes, and a test states which:

* the **private runtime** (`--no-scala-library`), where scala-rs emits its own
  minimal runtime classes;
* the **`--scala-library` dual run**, compiled against the real
  `scala-library-2.13.16.jar` and run as `java -cp out:scala-library.jar Main`
  (a `compile` with no flag auto-detects the jar); the
  `scala_library_dual_run_*` tests in `crates/cli/tests/e2e.rs` are the
  authoritative list for that file;
* a direct comparison with **real scalac** on every run where `scalac` is
  available (tests named `real_scalac_dual_run_*`, `scalac_agrees_*` or
  `*_matches_real_scalac`).

Programs run under `java -Xverify:all`. A fixture that uses library-only API
is run in jar mode only, with a `*_without_library_is_error`-style test
pinning that `--no-scala-library` still diagnoses it rather than quietly
accepting. Every positive fixture should have a `_bad` counterpart that pins
what the change must *not* accept, with the diagnostic real scalac gives, in
both modes. Multi-file programs live in `tests/multi/` (compiled together, and
where it matters also with the file order permuted); `tests/conform/` and
`tests/conform_multi/` hold differential programs that
`crates/cli/tests/conform.rs` runs under both compilers, requiring identical
stdout. Where a claim is about bytecode rather than output, pin it with
`javap -p -c -s`, since some defects (double boxing, a missing `Signature`
attribute) are invisible at run time.

Pickle reading and writing have their own tests:
`crates/pickle/tests/lib_jar.rs` scans every class file of the real
scala-library jar (and skips when the jar is absent), and
`crates/backend/tests/pickle_roundtrip.rs` reads back pickles our own writer
produced.

### Compile measures

`tests/slick_measure.sh`, `tests/cats_measure.sh`,
`tests/gitbucket_measure.sh` and `tests/scalalib_measure.sh` compile the
pinned projects and print one line of counts (`files`, `errors`,
`files_with_errors`, `classes`, `compiler_exit`). Exit 1 with actual
diagnostics is a valid measurement of unsupported sources; abnormal exits, a
failure without diagnostics, no source files, and a supposedly successful
build with no classes fail the measurement. These are not zero-error results.
Each writes per-invocation logs; set its `*_LOG` variable to keep one, and
`SCALA_RS=<binary>` to measure a binary without rebuilding. See
[cats.md](cats.md), [gitbucket.md](gitbucket.md) and
[scala-library.md](scala-library.md) for each project, and
`tests/scalalib_probe.sh` for probing inside the library.

`bash tests/measurement_harness_test.sh` checks these result classifications
(16 shell cases and six `tests/verify_all.sh` cases) without rebuilding
scala-rs or rerunning anything.

### Execution harnesses

Compiling without errors does not show that the output runs.
`tests/slick_run.sh`, `tests/cats_run.sh` and `tests/gitbucket_run.sh` are the
differential *execution* harnesses: each compiles its project with scala-rs
and with real scalac, compiles the client programs in `tests/slick_progs/`,
`tests/catsrun/` or `tests/gbrun/` with real scalac, runs them against both
builds and compares their stdout. `cats_run.sh` and `gitbucket_run.sh` compile
each client against both builds, which separates a codegen defect from a
pickle defect; `slick_run.sh` compiles each client once (`MODE` picks the
build on its compile classpath) and requires the compilation itself to
succeed. Each takes program names as arguments (all programs when none are
given); the script headers list their environment variables.

`tests/rt_probe.sh [name-regex]` compiles every `tests/rtprobe/*.scala`
program with scala-rs and with real scalac, runs both, and reports `ok`,
`MISCOMPILE`, `RUNTIME-ERR`, `WRONG-ACCEPT`, `reject` or `bad-probe` per
program. Programs named `neg_*.scala` must be rejected by both. It is the
check that finds defects that produce no diagnostic; run it for any change to
member resolution or code generation.

### What each check proves

Every check stops somewhere, and a green result says nothing past that point:

| check | what it reads | what it cannot see |
| --- | --- | --- |
| compile measures | diagnostics and class-file counts | anything about the emitted code |
| `tests/classfile_lint.py <dir>…` | `javap -c -p`: branch targets and method sizes | types, stack depth, stack map frames |
| `tests/slick_subset.sh` | loads every emitted slick class with `Class.forName(name, false, …)` | method bodies: an unlinked class is never verified |
| `tests/verify_all.sh <dir> [cp…]` | links every class with `Class.forName(name, true, …)` under `-Xverify:all` | behaviour of code that verifies |
| execution harnesses, `rt_probe.sh` | the output of the code the programs call | code no program calls |

`classfile_lint.py` reports a branch that leaves its own method (a wrapped
offset shows up as a negative or absurd target) or a method whose code is
65536 bytes or longer, and prints `lint_classes=N lint_problems=M`; it runs
inside `slick_run.sh` and `slick_subset.sh`.

`tests/verify_all.sh` reports `verify_classes`, `verify_failures`,
`verify_loaded` and `verify_incomplete`. Missing dependencies and failed
static initializers print `INCOMPLETE` rather than disappearing from the
result. Exit 0 means every discovered class loaded successfully; exit 1 means
a verification/class-format failure; exit 2 means coverage is incomplete or
the directory is empty. An incomplete load is not itself proof of a compiler
defect: supply the missing runtime classpath and inspect initialization
failures before making that diagnosis. The isolated loader delegates JDK
classes to the platform loader so that modules such as `java.sql` remain
available. Set `VERIFY_VERBOSE=true` to include exception causes and verifier
details in the log. A verification failure is a lower bound: a mis-resolved
call that happens to name a valid target verifies, so look for sibling cases
that do.

One check worth reusing that no harness runs: generic `Signature` attributes
are read by nothing above. Loading the emitted classes and asking
`java.lang.reflect` for the generic form of every member
(`Method#toGenericString`, `Field#getGenericType`, …) is the only way to find
a malformed one.

### Corpus, specialization and testkit

scala/scala's own corpus (`test/files/{pos,neg,run}`, 5324 programs at tag
`v2.13.16`) is run by `tests/scala_corpus.sh` and compared by test identity
with `tests/compare_corpus.py`; see [scala-corpus.md](scala-corpus.md).

`tests/spec_classfiles.sh` is the **specialization ledger**: it compiles each
of the corpus's `pos/spec-*` tests with real scalac and with scala-rs and
diffs the two sets of class-file *names*. It exists because accepting
`@specialized` raises the corpus's `pos` number by more than it earns, and a
number that goes up needs a number beside it that cannot. It compares names
only — nothing is loaded, verified or run. See
[specialization.md](specialization.md).

slick's own test suite (`slick-testkit`) is measured by
`tests/testkit_measure.sh`; see [slick-testkit.md](slick-testkit.md).

### The merge gate

The merge gate is `tests/verify_merge.sh`. It builds once, runs the four
compile measures and `slick_run.sh`, then `slick_subset.sh`, `cats_run.sh`,
`gitbucket_run.sh`, the workspace suite and the full corpus (concurrently
unless `GATE_SERIAL=1`), then `cargo fmt --all --check`. It ends with one
`VERDICT=PASS|FAIL` line and a `DONE` sentinel, plus a per-step wall-time
table. Launch it detached with a private `GATE_DIR`, and pair it with a
waiter that polls for `DONE` and then reads the verdict, as its header shows;
a failed run nobody reads looks exactly like a pass. `GATE_SKIP` names steps
to leave out, and a skipped required step makes the verdict FAIL.

The gate requires zero workspace test failures and the numbers recorded in
`tests/BASELINE.md`, so on a base that is already red it reports FAIL for
reasons that are not the branch's (see the base-commit comparison above).
`tests/verify_merge_test.sh` checks the gate's baseline and environment
contracts without building anything. The current accepted numbers are the
latest gate section at the end of `tests/BASELINE.md`; update that file only
from a completed gate run.

### Acceptance criteria

The target is observable Scala 2.13.16 behaviour: source acceptance, useful
diagnostics, program output, and interoperability with separately compiled
Scala and Java code. A falling error count is evidence to investigate, not
the definition of correctness.

- Release workspace tests and focused positive/negative regressions pass.
  Formatting and lint introduce no new warnings.
- A source fix rejects invalid programs at the intended construct and accepts
  the corresponding valid form. Merely rejecting a negative test is not enough.
- ABI/code-generation fixes compare scalac-only execution with scala-rs-only
  execution and both directions of separate compilation where applicable.
  Exercise methods; class loading does not prove dispatch or access
  correctness.
- Slick, cats and gitbucket keep nonempty class output, pass structural
  checks and JVM verification, and all differential execution attempts agree.
  Report classes whose loading could not complete separately from successful
  verification.
- Corpus logs are complete for the selected population before comparing them.
  Investigate changed test identities and output, not just net totals.
- Performance fixes report the same source set, settings, result, and timing
  (see [performance.md](performance.md)). A compiler crash, timeout, or empty
  output is never `errors=0` success.

Working rules that keep those results trustworthy:

- Work in an isolated worktree and branch; never change another checkout.
  Put every writable measurement output under a directory of your own.
- Rebuild before invoking a compiler binary directly, and record which tree
  produced any binary passed as `SCALA_RS`.
- Track long commands by their exit status; the existence of a log is not
  evidence of completion.
- Commit the tree you tested. An edit made after testing invalidates the
  affected evidence.
- Keep unmeasured or deliberately red checks explicit; do not infer their
  results.

### History

Earlier versions of this page listed the fixtures of each slice one by one
(`gap_`, `boxed`, `numt`, `prod`, `trex`, `lc` and many more) and told how
`tests/verify_all.sh` came to exist. The fixture files and their tests are the
authoritative record now; the old text is in
`git log -p -- docs/testing.md`, and the former `docs/development-plan.md`
(merged into this page) is in `git log -p -- docs/development-plan.md`.
