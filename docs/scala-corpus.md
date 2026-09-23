# scala/scala's own test corpus

Where this compiler stands on the tests scalac is developed against:
`test/files/{pos,neg,run}` from [scala/scala](https://github.com/scala/scala).
`tests/conform/` holds differential probes we wrote by hand; this is 5324
programs somebody else wrote, with expected output, and it costs nothing to
keep re-running. scala-rs implements a subset, so much of the corpus is
expected to fail; the number is the product.

## The material

| | |
|---|---|
| Repository | `https://github.com/scala/scala` |
| Revision | **`3f6bdaeafde17d790023cc3f299b81eaaf876ca3`** (tag `v2.13.16`) |
| Checkout | `/tmp/scala-rs-corpus/scala` (`CORPUS_DIR` to move it) |
| Test units | 1859 `pos`, 1405 `neg`, 2060 `run` |

`v2.13.16` is pinned because it is the same release as the real scalac the
`conform` suite dual-runs against, so a disagreement is about us and not about
a version skew. The clone is `--depth 1 --filter=blob:none`, about 100 MB.

A *test unit* is either `<name>.scala` or a directory `<name>/` holding several
sources that are compiled together.

## Why not partest

partest is scala/scala's own runner. It needs sbt, a built compiler, and its
`partest-extras` jar on the classpath. The tests themselves are just `.scala`
plus `.check`, so `tests/scala_corpus.sh` reads them directly. That is faster,
has no build to keep alive, and is something we can reason about when a number
moves.

The price is that we are not bug-compatible with partest: no `test/files/filters`
output normalisation, no `.javaopts`, and no separate-JVM handling. The `neg`
`.check` files *are* compared, but by message head rather than by full text —
see [`neg`, against the `.check` text](#neg-against-the-check-text).

## Two things the corpus is not

* **There is not one `.flags` file left in 2.13.16** (nor a `.javaopts`). Per-test
  compiler options moved into the source as a scala-cli style header:

  ```scala
  //> using options -Xlint:option-implicit -Xfatal-warnings
  ```

  926 sources carry one (291 under `pos`, 459 under `neg`, 176 under `run`).
  Five stragglers still use the intermediate `//scalac: ...` spelling. The
  runner parses both.

* **477 of the sources are `.java`.** A test unit with a Java source next to it
  needs javac and a mixed compilation round; those are skipped, not failed.

## What "pass" means

| | pass when |
|---|---|
| `pos` | scala-rs compiles the sources with zero errors **and emits at least one classfile** |
| `neg` | scala-rs reports at least one error |
| `run` | it compiles, `java Test` exits 0, and stdout matches the `.check` |

The `neg` rule in that table is an **upper bound**, and it is the number the
pass/fail column of the log still carries. A `neg` pass under it can be for the
wrong reason — a parse error where scalac reports a type error counts. The log
also carries both sides' diagnostics, and the report scores the wording on top;
see [`neg`, against the `.check` text](#neg-against-the-check-text).

`errors=0` is not enough for `pos`: a compiler that fell over quietly also
reports no errors. The classfile count is the same second reading the
slick and gitbucket measurements insist on.

### What counts as a skip, not a failure

Skips are things the runner has decided are not ours to judge. They are excluded
from the denominator.

| reason | meaning |
|---|---|
| `unsupported-flag <opt>` | the options header asks for a flag we do not implement (`-Xlint:*`, `-opt:*`, `-Xplugin:*`, `-Ystop-after:*`, …), which changes what scalac accepts or reports. `-Xsource:3`, `-Xsource:3-cross`, `-Xfatal-warnings`, `-Xasync`, `-language:*`, `-Xsource-features:*`, `-deprecation`, `-feature`, `-nowarn` and `-unchecked` are passed through (`-Werror` as `-Xfatal-warnings`); `-usejavacp` and `-explaintypes` are dropped |
| `java-sources` | a `.java` file belongs to the test |
| `needs-partest-or-junit` | the source imports `scala.tools.partest`, `scala.tools.nsc` or `org.junit` — it drives the compiler or a test framework we do not ship |
| `crash` | scala-rs panicked, overflowed its stack, or exited with something other than 0/1 |
| `timeout` / `run-timeout` | `CORPUS_TIMEOUT` (40 s) to compile, `CORPUS_RUN_TIMEOUT` (20 s) to run |
| `no-scala-sources` | a `.script` or `.pastie` unit |

A `crash` is a skip so that it does not silently inflate a `neg` pass rate, but
it is a defect.

## Why `pos` does not pass `-no-specialization`

`-no-specialization` is nsc's own flag, and it means *ignore the annotation* —
not *implement specialization*. nsc implements it: `@specialized` there means
`Foo$mcI$sp` classes get emitted and the ABI changes. Passing the flag to the
corpus would count "we ignored what the test was testing" as a pass, so the
corpus does not pass it.

`tests/cats_measure.sh` and `tests/scalalib_measure.sh` do pass it, because
they ask "where is type checking" rather than "is the ABI nsc's". The
annotation is now accepted and recorded
([specialization.md](specialization.md)), and when last compared the flag no
longer changed their error counts. Accepting the annotation is not
implementing the phase, though: `tests/spec_classfiles.sh` is the ledger that
compares the `$sp` classes scalac emits for `pos/spec-*` with ours.

## `neg`, against the `.check` text

The `.check` files say what scalac reports, down to the line and column. Full
text cannot be compared usefully:

* scalac splits a message over several lines — `type mismatch;` carries its
  `found`/`required` on continuation lines — while we print one line per
  diagnostic;
* the two type printers disagree on constants (`found : String("Hello")`
  against `found: "Hello"`), which measures the printer, not the type checker;
* the caret line and the column differ almost everywhere.

What *is* comparable is the **head** of the message: everything before the
first `;` and before the end of the first sentence, case- and whitespace-folded.
`tests/scala_corpus_report.sh` scores three tiers on it, each strictly inside
the previous one:

| | |
|---|---|
| **T1** | every diagnostic the `.check` expects has a match, as a multiset (four expected copies need four of ours), ignoring where it was reported |
| **T2** | … and each match is at the file and line scalac reports it at |
| **T3** | … and we emit nothing beyond the expected count |

Warning lines in a `.check` are used only when it holds no error line at all —
that is the shape of a test that fails because a warning was promoted. A head
match is not proof that the two compilers rejected for the same reason (two
different `type mismatch`es at the same line score as agreement), so T2 is a
much tighter bound than "any error", not an exact one.

## Running it

```
CORPUS_LOG=$MYDIR/corpus.tsv tests/scala_corpus.sh
```

Default is a **sample**: 250 tests per category, spaced evenly over the
alphabetical order so the same tests come back every run and two measurements
are comparable. The whole corpus is

```
CORPUS_LOG=$MYDIR/corpus.tsv CORPUS_SIZE=full tests/scala_corpus.sh
```

which takes many times longer. Do not run it next to a compile measurement:
a compile that exceeds its timeout under load is recorded as a skip.

| variable | default | |
|---|---|---|
| `CORPUS_LOG` | a shared scratchpad path | **override it** — the default is shared with every other agent |
| `CORPUS_SIZE` | `sample` | or `full` |
| `CORPUS_SAMPLE` | 250 | tests per category when sampling |
| `CORPUS_KINDS` | `pos neg run` | |
| `CORPUS_FILTER` | — | zsh glob matched against the test path, e.g. `'(t2973|u000a)'` |
| `CORPUS_JOBS` | 8 | parallel workers |
| `CORPUS_TIMEOUT` | 40 | seconds per compile |
| `CORPUS_RUN_TIMEOUT` | 20 | seconds per `java Test` |
| `CORPUS_DIR` | `/tmp/scala-rs-corpus/scala` | the checkout |

The log is one tab-separated line per test — `kind`, `name`, `pass`/`fail`/`skip`,
the first diagnostic verbatim, and for `neg` two more columns: every diagnostic
*we* produced and every diagnostic the `.check` expects. Both are lists of
`<file>:<line>: <level>: <message>` records joined by an ASCII record separator
(`\x1e`), so a test stays on one line and neither compiler's output can contain
the separator. Everything downstream is re-cuttable without re-running:

```
tests/scala_corpus_report.sh $MYDIR/corpus.tsv [top-N]
```

`scala_corpus.sh` calls the report itself unless `CORPUS_NO_REPORT` is set.

## Comparing two runs

Aggregate pass counts hide a pass lost in one place and gained in another.
Compare by test identity:

```
python3 tests/compare_corpus.py <baseline.tsv> <candidate.tsv>
```

It exits 1 on a lost pass or a newly skipped test and 2 on malformed or
incomparable ledgers, and prints the details as JSON. The accepted ledgers are
in `tests/baselines/`; `tests/BASELINE.md` names the current one. The merge
gate (`tests/verify_merge.sh`) runs the full corpus with longer timeouts and
compares it this way; a loss fails the gate unless the test passes on the
gate's serial re-run of just that test.

## Where we stand

The accepted numbers are in `tests/BASELINE.md`, not here. At gate `ff08907d`
(2026-09-14) the full corpus passes **1251 `pos` / 813 `neg` / 1018 `run`** of
the same 5324 identities, and the saved ledger is
`tests/baselines/corpus-ff08907d.tsv`.

The remaining failures fall into kinds that need different work, and
`tests/scala_corpus_report.sh` buckets them by first diagnostic:

* `pos` — programs scalac compiles and we reject. The tail is flat: after the
  specialization tests, most buckets are a handful of tests with unrelated
  roots.
* `neg` — programs scalac rejects and we accept (a missing rejection rule), and
  programs we reject for a different reason than scalac (visible in T1–T3).
  Adding a rejection rule is measured against slick, cats, gitbucket and the
  library before it is accepted, because a new rule tends to reject legal code
  too.
* `run` — programs that compile and then fail to link or verify, that print the
  wrong answer, or that need a feature we do not implement. The first two are
  the most serious, since a user gets wrong behaviour with no diagnostic.
* specialization — `tests/spec_classfiles.sh` records the classes scalac emits
  for `pos/spec-*` and we do not; see [specialization.md](specialization.md).

Pick one reproduced cause per change; a bucket's test count is an upper bound
on what fixing it yields, not a prediction.

## Known limits of this runner

* The `neg` pass/fail column is still "any error"; the wording comparison is a
  separate set of numbers in the report.
* Directory tests are compiled as one round unless the sources are named
  `..._1.scala`, `..._2.scala`; then they are compiled in numbered rounds with
  each round's output on the next round's classpath. partest's finer grouping
  rules are not reproduced.
* No `test/files/filters` normalisation, so a `run` `output-mismatch` can be a
  difference partest would have filtered away.
* `run` compares stdout, or stdout and stderr concatenated, against the
  `.check`. partest merges the two streams in real order; a test that
  interleaves them can be scored as a mismatch here.
* A `.java` beside a test is a skip; wiring in `javac` would recover those
  units.

## History

Earlier versions of this page recorded dated surveys: per-bucket failure
tables, the classification of the `run` failures, and per-slice write-ups
(cycle detection, the `ClassTag` and value-class rules, the `neg` wording
numbers). Code comments and tests that cite those sections refer to that
record; read it with `git log -p -- docs/scala-corpus.md`.
