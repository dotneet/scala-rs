# scala/scala's own test corpus

## Why `pos` does not pass `-no-specialization`

70 of `pos`'s 534 failures are `unimplemented syntax: annotation specialized`,
and `-no-specialization` would turn all of them green. The corpus does not pass
it, on purpose.

`-no-specialization` is nsc's own flag, and it means *ignore the annotation* —
not *implement specialization*. nsc implements it: `@specialized` there means
`Foo$mcI$sp` classes get emitted and the ABI changes. Passing the flag here
would count "we ignored what the test was testing" as a pass.

`tests/cats_measure.sh` and `tests/scalalib_measure.sh` do pass it, and that is
also on purpose: those two ask "where is type checking", a single parse error
aborts the whole run, and both codebases annotate everywhere. Without the flag
cats reports 71 errors and the library reports 84 — numbers that mean "nothing
was typechecked", not "almost nothing is wrong". The flag buys a meaningful
type-checking number at the cost of an ABI that differs from nsc's, which is
the right trade for a progress measure and the wrong one for a conformance
score.

So: 70 `pos` tests stay red until specialization is actually implemented. That
is the honest reading.

Where this compiler stands on the tests scalac is developed against:
`test/files/{pos,neg,run}` from [scala/scala](https://github.com/scala/scala).
This is a survey, not a campaign. `tests/conform/` is 86 differential probes we
wrote by hand; this is 5324 programs somebody else wrote, with expected output,
and it costs nothing to keep re-running.

scala-rs implements a subset, so most of the corpus is expected to fail. The
number is the product.

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

## Two things the corpus is not, contrary to what we assumed

* **There is not one `.flags` file left in 2.13.16** (nor a `.javaopts`). Per-test
  compiler options moved into the source as a scala-cli style header:

  ```scala
  //> using options -Xlint:option-implicit -Xfatal-warnings
  ```

  926 sources carry one (291 under `pos`, 459 under `neg`, 176 under `run`).
  Five stragglers
  still use the intermediate `//scalac: ...` spelling. The runner parses both.

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
wrong reason — a parse error where scalac reports a type error counts. Since
2026-09-05 the log also carries both sides' diagnostics and the report scores
the wording on top; the two numbers are printed side by side and neither
replaces the other. See [`neg`, against the `.check` text](#neg-against-the-check-text).

`errors=0` is not enough for `pos`: a compiler that fell over quietly also
reports no errors. The classfile count is the same second reading the
slick and gitbucket measurements insist on.

### What counts as a skip, not a failure

Skips are things the runner has decided are not ours to judge. They are excluded
from the denominator.

| reason | meaning |
|---|---|
| `unsupported-flag <opt>` | the `//> using options` header asks for a flag we do not implement. `-Werror`, `-Xlint:*`, `-opt:*`, `-Xplugin:*`, `-Ystop-after:*` all change what scalac accepts or reports, so the test is not a fair question. `-Xsource:3`, `-Xfatal-warnings`, `-language:*`, `-Xsource-features:*`, `-Xasync` are passed through; `-deprecation`, `-unchecked`, `-feature`, `-nowarn`, `-explaintypes` are dropped as harmless |
| `java-sources` | a `.java` file belongs to the test |
| `needs-partest-or-junit` | the source imports `scala.tools.partest`, `scala.tools.nsc` or `org.junit` — it drives the compiler or a test framework we do not ship |
| `crash` | scala-rs panicked, overflowed its stack, or exited with something other than 0/1 |
| `timeout` / `run-timeout` | 40 s to compile, 20 s to run |
| `no-scala-sources` | a `.script` or `.pastie` unit |

A `crash` is a skip so that it does not silently inflate a `neg` pass rate, but
it is a defect: see the counts below.

## Running it

```
CORPUS_LOG=$MYDIR/corpus.tsv tests/scala_corpus.sh
```

Default is a **sample**: 250 tests per category, spaced evenly over the
alphabetical order so the same tests come back every run and two measurements
are comparable. Under a minute (42 s at `CORPUS_JOBS=6`), and close enough to
be useful — the same tree that scores 63.9 / 61.4 / 28.7 on the whole corpus
scores 65.5 / 56.7 / 28.2 on the sample. The whole corpus is

```
CORPUS_LOG=$MYDIR/corpus.tsv CORPUS_SIZE=full tests/scala_corpus.sh
```

and takes ten to fifteen minutes at `CORPUS_JOBS=6`. Do not run it next to a
slick measurement.

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

## Where we stand

Measured on `agent/scalacorpus` merged with `main` at `10bd2d5`, 2026-09-05,
whole corpus, `CORPUS_JOBS=6`, about fifteen minutes.

| | total | pass | fail | skip | pass rate (of non-skipped) |
|---|---|---|---|---|---|
| `pos` | 1859 | 965 | 545 | 349 | **63.9 %** |
| `neg` | 1405 | 634 | 399 | 372 | **61.4 %** |
| `run` | 2060 | 433 | 1074 | 553 | **28.7 %** |
| all | 5324 | 2032 | 2018 | 1274 | 50.2 % |

The breakdowns in this section are all from that run. The numbers themselves
have since moved — `main` at `d4131b0` scores 974 / 634 / 434 pass, and the
cycle-detection slice below takes `pos` to 977 and `neg` to 640 — but the
shape of the tail has not, so the tables are left as they were measured
rather than half-updated.

### `pos` — 545 programs scalac compiles and we do not

| count | first diagnostic |
|---|---|
| 97 | `type mismatch` |
| 70 | `unimplemented syntax: annotation specialized` |
| 5 | `whitebox macros are not implemented` |
| 5 | `not found: type TypeTag` |
| 5 | `no implicit: could not find implicit value of type ...` |
| 5 | `expected =, found lbrace` |
| 4 | `object creation impossible.` |
| 4 | `not found: type Manifest` |
| 4 | `no matching overload for constructor Seq` |
| 4 | `expected expression, found comma` |

The tail is long and flat: past the two big rows, 545 failures spread over some
300 distinct first diagnostics. There is no single missing feature here, which
is itself the finding — `pos` is measuring the width of the subset, not one hole.

`@specialized` is the largest *single* cause at 70 `pos` plus 34 `run`, and it is
a deliberate refusal (`annotation_compiler_unsupported` in `crates/parser/src/parse.rs`),
not a bug. Specialization changes the classes and signatures that come out, so
accepting and ignoring it would be a stub. It is worth knowing that one
deliberate diagnostic costs about 104 corpus tests.

### `neg` — 399 programs scalac rejects and we accept

This is the more serious column, and the `.check` files say exactly which check
we are not performing:

| count | what scalac says |
|---|---|
| 26 | `type mismatch;` |
| 8 | `double definition:` |
| 6 | `match may not be exhaustive.` |
| 5 | `pattern type is incompatible with expected type;` |
| 5 | `incompatible type in overriding` |
| 5 | `ambiguous reference to overloaded definition,` |
| 4 | `unreachable code` |
| 4 | `name clash between defined and inherited member:` |
| 4 | `The outer reference in this type test cannot be checked at run time.` |
| 4 | `No ClassTag available for T` |
| 3 | `patterns after a variable pattern cannot match (SLS 8.1.1)` |
| 3 | `macro implementation has incompatible shape:` |
| 3 | `illegal inheritance;` |
| 3 | `encountered unrecoverable cycle resolving import.` |
| 3 | `Companions X and X must be defined in same file:` |

The 26 `type mismatch` misses are not one root. Four sampled by hand came out
as four different holes:

* `neg/unit2anyref` — `val x: AnyRef = ()` is accepted. So is `val x: AnyRef = 1`.
  Conformance lets a primitive and `Unit` widen to `AnyRef`, which nsc does not.
* `neg/val_infer` — an inferred override result type is not checked against the
  base declaration (`def foo = ""` overriding `def foo: Int`).
* `neg/t909` — a constant pattern's type is not checked against the scrutinee
  (`case Foo("Hello")` against `Foo(x: Int)`).
* `neg/sip23-null` — `null` is accepted where a singleton type `x.type` is required.

The `neg` *passes* need the same scepticism, which is why the report prints
them by diagnostic: 74 of the 634 reject with a `type mismatch`, but a good
number of the rest reject because of an unrelated hole (`not found: type TypeTag`,
`unimplemented syntax: ...`) rather than the error the test is about. The `neg`
number is an upper bound on our real rejection conformance; the `.check` text
has since been compared and says how far off it is — 640 down to 99. See
[`neg`, against the `.check` text](#neg-against-the-check-text).

### `run` — 1074 failures, in three quite different kinds

| count | |
|---|---|
| 786 | does not compile |
| 194 | compiles, then the JVM or the test's own assertion rejects it |
| 94 | compiles and runs, prints something else |

The 194 are the alarming ones — bytecode we emit that does not do what it says:

| count | |
|---|---|
| 47 | `java.lang.VerifyError` |
| 32 | `java.lang.NoSuchMethodError` |
| 28 | `java.lang.AssertionError` (the test's own `assert`) |
| 18 | `java.lang.ClassCastException` |
| 15 | main method not found in class `Test` |
| 9 | `java.io.NotSerializableException` |
| 7 | `java.lang.NoClassDefFoundError` |
| 7 | `java.lang.AbstractMethodError` |
| 4 | `java.lang.IllegalAccessError` |

The 15 "main method not found" share one root, and it is small: **we do not emit
static forwarders into a companion class.** `run/t363.scala` is nothing but

```scala
object Test { def main(args: Array[String]): Unit = println("...") }
class Test { def kurtz() = "..." }
```

nsc puts a `public static void main(String[])` forwarder in `Test.class` when
the module has a companion class (it only emits a separate mirror class when
there is none). We emit `Test.class` with just `kurtz`, so `java Test` cannot
start. Reproduced directly with `javap`; it is not an artefact of the runner.

### Eight stack overflows — fixed, see below

Not counted as failures — they are skips, because a crash must not quietly
inflate a `neg` pass rate — but they were the clearest defect the corpus found:

```
neg/t10530  neg/t2918  neg/t5093  neg/t5878
pos/matthias4  pos/t1357  pos/t2994a  pos/t690
```

All eight are cyclic type references, and the four `neg` ones are tests whose
whole point is that scalac says `illegal cyclic reference involving type A`
(`neg/t2918` is two lines: `def g[X, A[X] <: A[X]](x: A[X]) = x`). There was no
cycle detection in type resolution, so we recursed until the stack ended. This
is the same failure mode as the `SymbolTable::lub` overflow that made a
gitbucket measurement report `errors=0 classes=0`.

There were no timeouts at 40 s.

## Cycle detection (2026-09-05)

`crates/typer/src/cyclic.rs` and `symbol::enter_chase` closed all eight. The
corpus is now free of crashes.

### What the eight actually were, and they were not one bug

| | |
|---|---|
| `neg/t2918`, `neg/t5093` | a type parameter bounded by itself, `A[X] <: A[X]`. `erase_ty` ↔ `widen_type_param` and `class_sym_of` both looped |
| `neg/t5878`, `neg/t10530` | value classes that wrap each other. A value class erases to what it wraps, so the pair has no erasure and `erase_ty` unboxed one into the other for ever |
| `pos/t1357` | a recursive existential (`T forSome { type T <: Tuple2[BT[E, T], BT[E, T]] }`) reached through a `Tuple` alias, whose erasure *does* visit its arguments |
| `pos/t690`, `pos/matthias4` | `class_sym_of` following an abstract member's bound back to itself |
| `pos/t2994a` | Peano naturals: `type a[s[_], z] = s[n#a[s, z]]` grows one layer per higher-kinded expansion |

### The rules, all read off `/tmp/scala-2.13.16/bin/scalac`

nsc marks a symbol `LOCKED` while it completes it and raises `CyclicReference`
on re-entry. Two halves of that are reproduced.

**Bounds.** An *upper* bound whose **head** is the type it bounds is
`cyclic aliasing or subtyping involving type X`. Heads only: the walk steps
through an application, an annotation and the parents of a compound, and stops
at a class — which is what keeps F-bounded polymorphism (`trait Ord[A <: Ord[A]]`)
and `type X <: List[X]` legal, both of which scalac accepts. Aliases
(`type U = U`, `type X = List[X]`) were already covered by
`check::expand_one_alias`.

The two bounds are **not** symmetric, and reading them the same way cost
`pos/contrib701` before the difference was probed:

```scala
trait B { type A[T] >: A[A[T]] }      // accepted — this is all of pos/contrib701
trait B { type A    >: A       }      // illegal cyclic reference involving type A
trait B { type A[T] <: A[T]    }      // cyclic aliasing or subtyping involving type A
```

An *applied* self-reference is a cycle in the upper bound and not in the lower
one, so the lower bound only counts a **bare** self-reference, and it carries
nsc's other message.

**Value classes.** `value class may not wrap another user-defined value class`,
nsc's `validateDerivedValueClass`. The predicate was probed rather than
assumed: a compound counts when *any* parent is a value class (`Tr with VA` as
well as `VA with Tr`), and a type parameter counts when its upper bound is one
(`class B[T <: A](val a: T) extends AnyVal`), while `Tr with Int` does not.

**The walks defend themselves.** `class_sym_of`, `widen_type_param`,
`erase_ty` and `expand_applied_hk_alias` all replace an abstract type by what
it stands for. `symbol::enter_chase` is `LOCKED` for those four: re-entry
answers "no more information" rather than raising, because erasure also runs
over signatures the typer never checked — a pickle or a class file can carry a
cycle nobody in this compilation wrote. The four are kept apart by a `Chase`
tag; `class_sym_of` looking through `X` while erasure is unfolding `X` is not
a cycle and must not be told that it is.

`lub_at`'s depth cap of 6 was left alone. It is not a stand-in for cycle
detection: nsc bounds the same recursion with `Depth`/`maxDepth` and answers
`Any` when it runs out, and what grows there is the type *arguments*, not a
symbol that repeats — a symbol-keyed guard would never fire.

### What it moved

Whole corpus, before and after, on the same tree otherwise (`main` at `d4131b0`
merged in):

| | pass | fail | skip | rate |
|---|---|---|---|---|
| `pos` before | 974 | 536 | 349 | 64.5 % |
| `pos` after | **977** | 537 | 345 | **64.5 %** |
| `neg` before | 634 | 399 | 372 | 61.4 % |
| `neg` after | **640** | 397 | 368 | **61.7 %** |
| `run` before/after | 434 | 1073 | 553 | 28.8 % |

Twelve tests changed status and nothing regressed:

```
neg/t10530  skip -> pass   value class may not wrap another user-defined value class
neg/t2918   skip -> pass   cyclic aliasing or subtyping involving type A
neg/t5093   skip -> pass   cyclic aliasing or subtyping involving type C
neg/t5878   skip -> pass   value class may not wrap another user-defined value class
neg/t6337   fail -> pass   value class may not wrap another user-defined value class
neg/t798    fail -> pass   cyclic aliasing or subtyping involving type Bracks
pos/cls1    fail -> pass
pos/t1090   fail -> pass
pos/t1357   skip -> pass
pos/matthias4  skip -> fail   type AObject is not a member of <notype>
pos/t2994a     skip -> fail   incompatible type in overriding type a
pos/t690       skip -> fail   incompatible type in overriding type T
```

The three `pos` rows that went `skip -> fail` are **not** a regression: they
were crashes, and a crash is excluded from the denominator while a failure is
not. Each is now a diagnostic on a program scalac accepts, which is a hole to
narrow rather than a compiler that dies. They are three different holes — a
path-dependent `val a: _a; type A <: a.AObject` prefix, and two as-seen-from
bugs — and none of them is cycle detection.

`pos/cls1` and `pos/t1090` came from the one real bug this slice turned up:
`term_path_type` read `Outer.this` as plain `this`, so
`trait Outer { type T; trait Inner { type T <: Outer.this.T } }` bounded
`Inner`'s own `T` by itself. That invented cycle is why `pos/t690` overflowed;
with the qualifier honoured, the shape compiles.

slick (`files=184 errors=0 files_with_errors=0 classes=1596`), `slick_run`
(`progs=12 ok=12 diff=0 fail=0`), cats and gitbucket
(`errors=1859 files_with_errors=186`) are byte-identical before and after,
which is the check that mattered: this slice adds two rejection rules.

The cats pair was measured at `errors=71 files_with_errors=16` on both sides,
before `tests/cats_measure.sh` started passing `-no-specialization`; that 71
was a parse abort and not a type-checking figure (see the note at the top of
`docs/cats.md`). Re-measured with the current script on the merged tree, cats
is `errors=2929 files_with_errors=151` — main's own recorded number, to the
error.

## `neg`, against the `.check` text

### Why the old number was an upper bound, and what replaces it

`neg` passed on "scala-rs reported at least one error". 640 of 1037 non-skipped
tests did, 61.7 %. That counts a rejection for the wrong reason, and the wrong
reason is common: **98 of those 640 were rejected by a parse error or an
explicit "unimplemented" refusal**, so the program never reached the check the
test exists to exercise.

The `.check` files say what scalac reports, down to the line and column. Full
text cannot be compared, and it is worth being precise about why rather than
calling it "close enough":

* scalac splits a message over several lines — `type mismatch;` carries its
  `found`/`required` on continuation lines, `match may not be exhaustive.`
  carries "It would fail on the following input" on the next — while we print
  one line per diagnostic. The line structure is not a difference in what was
  checked.
* the two type printers disagree on constants: scalac writes
  `found : String("Hello")`, we write `found: "Hello"`. Comparing that tail
  measures the printer, not the type checker.
* the caret line and the column differ almost everywhere.

What *is* comparable is the **head** of the message: everything before the
first `;` and before the end of the first sentence, case- and whitespace-folded.
Three tiers are scored on it, each strictly inside the previous one:

| | |
|---|---|
| **T1** | every diagnostic the `.check` expects has a match, as a multiset (four expected copies need four of ours), ignoring where it was reported |
| **T2** | … and each match is at the file and line scalac reports it at |
| **T3** | … and we emit nothing beyond the expected count |

Warning lines in a `.check` are used only when it holds no error line at all —
that is the shape of a test that fails because a warning was promoted. Taking
warnings *alongside* errors would score us on lints nobody claims we implement.

### The numbers

Whole corpus at `main` `1a494fb`, 2026-09-05, `CORPUS_KINDS=neg CORPUS_SIZE=full`,
1405 units, 368 skipped, 1037 judged:

| | count | of 1037 |
|---|---|---|
| **T0** any error at all — the old rule | **640** | **61.7 %** |
| **T1** expected messages reproduced | **104** | **10.0 %** |
| **T2** … at the expected file and line | **99** | **9.5 %** |
| **T3** … and nothing extra | **79** | **7.6 %** |

Both ends of that are real. T0 says we reject 640 programs that must be
rejected; T2 says we reject 99 of them *for the reason the test is about*. The
gap, 541 tests, is the size of the accounting error the old number carried.

Five tests have no `error:` or `warning:` line in their `.check` at all and are
left out of T1–T3 while staying in the 1037 and in T0.

### Where the other 938 go

| count | |
|---|---|
| 380 | **a** — we accept the program; no diagnostic at all |
| 472 | **b** — we reject it, but for none of the expected reasons |
| 76 | **c** — partial: some of the expected diagnostics reproduced |
| 5 | **d** — right messages, wrong file or line |
| 20 | **e** — right messages and lines, plus extra of our own |
| 79 | **f** — exact match |

(T2 = 99 is d + e + f; T3 = 79 is f. The five `.check`-less tests are not in
this table.)

**b is the interesting one and it has no shape.** 472 tests, **341 distinct
first diagnostics** on our side and 345 distinct on scalac's. The largest single
row is 17. 83 of the 472 are a parse or syntax refusal from us — the test is
rejected before type checking starts.

What we said instead of what was expected, most frequent first:

| count | scalac expects | we said |
|---|---|---|
| 5 | `the splice cannot be resolved statically` | `value currentMirror is not a member of package scala.reflect.runtime` |
| 4 | `forward reference to value a extends over definition of value b` | `not found: value a` |
| 3 | `no TypeTag available for T` | `not found: type TypeTag` |
| 3 | `incompatible type in overriding` | `type mismatch` |
| 3 | `missing parameter type` | `missing parameter type for expanded function` |
| 3 | `expected class or object definition` | `expected newline or \`` |
| 3 | `type mismatch` | `implicit conversion method foo1 should be enabled by making the implicit value visible` |

Only the last three rows are "the same check, different words". The rest are a
different check firing first.

**a — the 380 programs we accept.** Bucketed by what scalac says, this is the
list of checks we do not perform, and it is the same flat tail the earlier
survey found: `type mismatch` 21, `double definition:` 8,
`match may not be exhaustive` 6, `ambiguous reference to overloaded definition,`
5, `incompatible type in overriding` 5,
`pattern type is incompatible with expected type` 5,
`no ClassTag available for T` 4, `unreachable code` 4,
`the outer reference in this type test cannot be checked at run time` 4,
`name clash between defined and inherited member:` 4, then singletons.

**c — 26 of the 76 partials are a subset, not a disagreement.** Everything we
say is a diagnostic the `.check` expects; we just say it fewer times.
`neg/accesses` expects four `weaker access privileges in overriding` and gets
one; `neg/cyclics` expects three `illegal cyclic reference` and gets the first;
`neg/t3481` expects five `type mismatch` and gets two. These are not one root —
they are three different places where the first error of a kind suppresses the
rest — but they are the cheapest tests to move, because the check itself is
already implemented and correct.

**e — 20 tests fail only T3**, because we emit more diagnostics than the
`.check` has. That is cascade, not a missing check.

### What this says to fix next

The honest headline is that **there is no big lever here**. The `neg` tail is as
flat as the `pos` tail: ~340 distinct wrong reasons over 472 tests. In rough
order of cost per test moved:

1. **Report every occurrence of a check, not the first** — 26 tests in bucket
   c, and the checks already exist. Three or four separate suppression sites.
2. **Cascade suppression** — 20 tests in bucket e, T3 only.
3. **The 98 T0 passes that are parse or "unimplemented" refusals** are noise in
   the headline number rather than a fix; knowing they are there is the point.
4. Everything else is one test at a time.

## Three checks we were not performing (2026-09-05, `agent/accepttoomuch`)

Bucket **a** — the 380 `neg` tests we compiled without a word — was the target.
It is now 364, and the three holes closed were worth more than that count
suggests, because two of them were holes in *every* program rather than in one
check.

### 1. A written type annotation naming nothing

`def f(x: Zork): Int = 3` compiled. So did `val x: Zork`, `def f(x: Int): Zork`
and `def f(x: List[Zork])`. Only a template's parents, its self type and the
class a `new` builds resolved strictly (`Typer::strict_type_names`); everywhere
else an unresolved name stayed a `Type::Named` placeholder and the rest of the
run went on with it. `type_val_sig` and `type_def_sig` now resolve under the
same flag.

Three things had to be true for that not to break working code, and each one
was found by a measurement rather than by reading:

* **An existential binds its own names.** `subst_quantified` runs *after* the
  body is resolved, so `val x: A[X] forSome { type X }` has `X` standing for
  nothing while the body is being built. Six `pos` tests regressed on the first
  attempt (`exbound`, `depexists`, `t0905`, `t1048`, `t1560`, `t5022`). The
  quantified names are now announced before the clause is resolved.
* **A wildcard import whose members we cannot enumerate leaves the scope
  open.** gitbucket writes `import gitbucket.core.model.Profile.profile.blockingApi._`
  and then 259 signatures naming `Session`, a type member reached through that
  path. `import p._` is only enumerable when `p` is a package or an object; a
  prefix that did not resolve, or a *value* whose type is a jar class read one
  name at a time, is not. In such a file the rule stands down
  (`Typer::opaque_import_files`). Without it gitbucket went from 1693
  diagnostics to 2230.
* **nsc's error type is absorbing.** When the type *constructor* names nothing,
  its arguments are not reported as well — `-Ykind-projector` leaves an
  unrecognised `Functor[λ[α => Box[α], β]]` untouched, and `α`/`β` are then
  names nobody wrote a binder for. One diagnostic, not three.

### 2. A local `type` alias had no symbol at all

A block ran the namer over its `class` and `object` statements only, so

```scala
type Branches = List[(F[Boolean], F[A])]
def step(branches: Branches): F[Either[Branches, A]] = ...
```

— cats' `Monad.ifElseM` — left `Branches` standing for nothing. That was
invisible while an unresolved name in a signature was tolerated. A block now
resolves its type aliases first, the way a template does, and **cats went from
1128 diagnostics to 1108 on the strength of that one fix**.

The pre-pass stops at the first `import`: an import inside a block takes effect
where it stands, and `pos/t5305` writes `import O.{F, v}` before
`type x = { type l = (F, v.type) }`.

### 3. Two overloads that erasure merges into one descriptor

nsc's `RefChecks.checkNoDoubleDefs`, now `crates/typer/src/double_def.rs`. Eight
`neg` tests, and eight class files we were emitting with two identical methods.

The rule is over the **descriptor**, and both halves of that were probed
against `/tmp/scala-2.13.16/bin/scalac` rather than assumed:

* parameter clauses are flattened, a repeated parameter is the `Seq` it
  becomes, a value class is what it wraps, and a singleton type is its
  underlying type — `neg/t6443c`, `neg/t0259`, `neg/valueclasses-doubledefs`,
  `neg/t8323`;
* the **result** type is part of it, and the JVM lets two methods differ in it
  alone. `scala.Function.uncurried` is five overloads that all take one
  `Function1`; scalac accepts those and rejects
  `def g(x: List[Int]): Int` beside `def g(x: List[String]): Int`. Leaving the
  result out of the key cost twelve false diagnostics on
  `src/library/scala/Function.scala` alone;
* a **macro def** has no bytecode, so two of them cannot collide. scalac
  accepts `pos/t7776`'s two `app` macros and rejects the same pair written as
  ordinary methods.

The check is deliberately narrow otherwise: only members the source of one
template wrote, never a synthetic, a bridge or an accessor, and never a
signature holding a `NoType`, an `Error` or an unresolved `Type::Named`.

### What it moved

Whole corpus, `main` at `74ed830` merged in, `CORPUS_SIZE=full CORPUS_JOBS=6`.
Both columns are that same merged tree against `main` alone at `74ed830`, so
the other slices that landed while this one ran are in *both* numbers:

| | before | after |
|---|---|---|
| `pos` pass | 977 | **980** |
| `neg` **T0** any error | 640 (61.7 %) | **656 (63.3 %)** |
| `neg` **T1** expected messages reproduced | 104 (10.0 %) | **115 (11.1 %)** |
| `neg` **T2** … at the expected line | 99 (9.5 %) | **110 (10.6 %)** |
| `neg` **T3** … and nothing extra | 79 (7.6 %) | **89 (8.6 %)** |
| bucket **a** — accepted with no diagnostic | 380 | **364** |

Nothing regressed in either column. The three `pos` gains are `generic-sigs`,
`t9326a` and `tcpoly_typesub`; the sixteen `neg` gains are `func-max-args`,
`overloaded-unapply`, `patmat-type-check`, `t0259`, `t1565`, `t2779`, `t3653`,
`t588`, `t6443c`, `t7602`, `t8300-overloading`, `t8323`, `t8890`,
`valueclasses-doubledefs`, `valueclasses-pavlov`, `volatile_no_override`.

The four project measurements, which are what a rejection rule has to be
judged on:

| | before | after |
|---|---|---|
| slick | `files=184 errors=0 files_with_errors=0 classes=1596` | identical |
| `tests/slick_run.sh` | `progs=12 ok=12 diff=0 fail=0` | identical |
| cats | `errors=1128 files_with_errors=141` | **1108 / 139** |
| gitbucket | `errors=1391 files_with_errors=184` | **1373 / 184** |
| `src/library` `-no-specialization` | `errors=1969 files_with_errors=172` | **1903 / 172** |

Three of the four went *down*, which is the shape a rejection rule should have
when the rejection is real and the compiler was hiding a resolution failure
behind a placeholder. Measured on the earlier base `cad281b`, where gitbucket
was 1693 and `src/library` 1997, the same three moved the same way (1675 and
1944 -- 1903 once the erased *result* joined the double-definition key).

### One thing this exposed and did not fix

**`scala.Singleton` is not resolvable.** It has no class file — the jar has
none, and `src/library-aux` is scaladoc-only — so it was one of the names that
survived as a placeholder. `val x: Singleton` is now `not found: type
Singleton`, which is honest but is a diagnostic scalac does not have. Five
`pos` tests report it (`scala-singleton`, `sip23-singleton-sub`,
`sip23-singleton-view`, `t4914`, `t7520`); all five were already failing for
the same underlying reason, so the pass rate did not move. It needs a compiler-
defined symbol, the way `Any`/`AnyRef`/`Nothing` have one.

## The `run` failures, classified (2026-09-05)

`run` is the weakest of the three categories — 444 of 2060 — and until now it
was the only one nobody had broken down by symptom. Here it is. The 1063
failures split into two piles that deserve very different priorities.

### Pile one: we are wrong (237)

These compile, they load, they run, and the answer is not scalac's. A program
we *reject* costs the user a diagnostic. A program we accept and get wrong
costs them a debugging session, and none of our other checks catch it.

| symptom | count | what it means |
| --- | ---: | --- |
| `output-mismatch` | 93 | ran to completion, stdout differs from `.check` |
| `AssertionError` | 28 | the test's own `assert` failed — same thing, louder |
| `VerifyError` (all shapes) | 44 | the JVM refuses the classfile we wrote |
| `ClassCastException` | 19 | erasure or a cast we inserted |
| `NoSuchMethodError` | 33 | the call site and the callee disagree |
| `AbstractMethodError` | 8 | a forwarder or bridge we did not emit |
| `IncompatibleClassChangeError` / `NoClassDefFoundError` | 5 | |

Two of these deserve a note. **`AbstractMethodError` and `NoSuchMethodError`
are invisible to the verifier** — a call through an interface type is not
type-checked by it — which is exactly how a value-class defect once walked past
all six detection methods and surfaced only at run time. And `output-mismatch`
is invisible to *everything* except running the program against expected
output, which is what this category exists to do.

### Pile two: we do not implement it (≈500)

| symptom | count |
| --- | ---: |
| `scala.reflect.runtime` surface (`currentMirror`, `runtimeMirror`, `TypeTag`, `Manifest`, `Universe#Transformer`) | ≈200 |
| `reify { … }` beyond literals | ≈90 |
| `@specialized` | 37 |
| whitebox macros | 12 |
| quasiquotes | ≈10 |

These are honest gaps with honest diagnostics. They are worth doing, and the
reflection block is nearly all supply — the jar is already on the corpus
classpath — but a missing feature that says so is not a defect in the sense
pile one is.

`553` more `run` tests are skipped rather than failed (unsupported harness
shapes: `.javaopts`, separate JVMs, `filters`); see the limits section below.

## What would move the number most

1. **Static forwarders into a companion class.** Fifteen `run` tests, one
   well-understood rule, contained to the backend.
2. **`AnyRef` conformance.** `val x: AnyRef = 1` compiling is a hole under
   everything else; but it is a *rejection* rule, and this project's history
   says a new rejection rule breaks more than it fixes. Do it with the slick,
   cats and gitbucket measurements in hand. (The three rules in
   [the section above](#three-checks-we-were-not-performing-2026-09-05-agentaccepttoomuch)
   were added that way and three of the four measurements improved — but two of
   them only after a guard that a measurement, not a reading of the code, said
   was needed.)
3. ~~**Compare the `neg` `.check` text.**~~ Done, 2026-09-05. The real figure is
   9.5 % (T2), not 61.7 %. What it turned up is that the `neg` tail is as flat
   as the `pos` one; the ranked follow-ups are at the end of
   [that section](#what-this-says-to-fix-next).
4. **The 237 in [pile one](#pile-one-we-are-wrong-237).** Programs we accept
   and then get wrong. This is now the top lever, ahead of anything that adds
   a feature: every one of them is a case where a user would ship the wrong
   behaviour with no diagnostic to warn them.

## Known limits of this runner

* The `neg` pass/fail column is still "any error"; the wording comparison is a
  separate set of numbers in the report, and it compares message *heads*, not
  full text. A head match is not proof the two compilers rejected for the same
  reason — two different `type mismatch`es at the same line score as agreement.
  It is a much tighter bound than "any error", not an exact one.
* Directory tests are compiled as one round unless the sources are named
  `..._1.scala`, `..._2.scala`; then they are compiled in numbered rounds with
  each round's output on the next round's classpath. partest's finer grouping
  rules are not reproduced.
* No `test/files/filters` normalisation, so a `run` `output-mismatch` can be a
  difference partest would have filtered away.
* `run` compares stdout, or stdout and stderr concatenated, against the
  `.check`. partest merges the two streams in real order; a test that
  interleaves them can be scored as a mismatch here.
* A `.java` beside a test is a skip; wiring in `javac` would recover 318 units.
