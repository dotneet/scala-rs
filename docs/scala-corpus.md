# scala/scala's own test corpus

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
output normalisation, no `.javaopts`, no separate-JVM handling, and no `.check`
comparison for `neg`.

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

The `neg` rule deliberately does **not** compare the `.check` text. The first
question is whether we reject what has to be rejected at all; matching scalac's
wording is a later slice. The consequence is that a `neg` pass can be for the
wrong reason — a parse error where scalac reports a type error still counts —
so the report prints which diagnostic did the rejecting, and that column is
worth reading before believing the `neg` number.

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
and the first diagnostic verbatim — so it can be re-cut without re-running:

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
number is an upper bound on our real rejection conformance until the `.check`
text is compared.

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

## What would move the number most

1. **Static forwarders into a companion class.** Fifteen `run` tests, one
   well-understood rule, contained to the backend.
2. **`AnyRef` conformance.** `val x: AnyRef = 1` compiling is a hole under
   everything else; but it is a *rejection* rule, and this project's history
   says a new rejection rule breaks more than it fixes. Do it with the slick,
   cats and gitbucket measurements in hand.
3. **Compare the `neg` `.check` text.** The 61.4 % `neg` figure is an upper
   bound: it counts a rejection for the wrong reason as a pass. Matching the
   message would turn the column into a real number, and the log already
   records which diagnostic fired.
4. **The 47 `VerifyError`s.** Every one is a classfile the JVM refuses. They
   need individual narrowing, but the corpus hands over the reproducers.

## Known limits of this runner

* `neg` is judged by "any error", not by the expected message (see above).
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
