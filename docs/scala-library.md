# The Scala standard library

Where this compiler stands on **its own standard library**: `src/library` of
[scala/scala](https://github.com/scala/scala), the sources the
`scala-library-2.13.16.jar` we link against is built from. Like `docs/cats.md`
and `docs/gitbucket.md` this is a survey — the point is to have the number and
the symptoms written down.

This one is different from the other benchmarks in kind, not only in size.
slick, cats and gitbucket are *users* of the standard library. `src/library`
**is** the standard library, and this compiler does not learn the standard
library from source: it has it built in, as the `crates/typer/src/prelude*.rs`
signature tables (and, in `--scala-library` mode, the jar). So compiling
`src/library` asks the compiler to typecheck source definitions of the very
names it already believes it knows. That collision, not any missing language
feature, is what the numbers below are made of.

## The material

| | |
|---|---|
| Repository | `https://github.com/scala/scala` |
| Revision | **`3f6bdaeafde17d790023cc3f299b81eaaf876ca3`** — tag `v2.13.16`, the same release as the jar the rest of the suite links against |
| Module | the `library` subproject's `Compile` configuration |
| Sources | **538 `.scala`** under `src/library`, plus 32 `.java` |
| Not compiled | `src/library-aux` (`Any`, `AnyRef`, `Nothing`, `Null`, `Singleton`) — `build.sbt` passes it to scaladoc as `-doc-no-compile`; those five are compiler built-ins |

The 32 Java sources (`BoxesRunTime`, `Statics`, the `*Ref` boxes, `BoxedUnit`,
`ScalaNumber`, the concurrent TrieMap bases, `ScalaSignature`) are javac's in
the real build. There is no Java front end here, so `tests/scalalib_measure.sh`
puts the 33 classfiles they produce — extracted from the released jar — on the
classpath instead.

### Flags

`build.sbt` gives the library project:

```
-feature -Xlint -sourcepath <src/library>
-Wconf:cat=unchecked&msg=The outer reference…:s -Wconf:cat=optimizer:is
-Wconf:cat=unused-nowarn:s -Wunnamed-boolean-literal-strict
```

plus `-Werror` when `fatalWarnings` is on (CI and release builds, not local
development). **None of these changes what is accepted**, so the measurement
passes none of them. In particular there is no `-Xsource:3`, no `-Yrecursion`
and no `-opt`: the optimiser is turned on only for the `bench` subproject and
for the bootstrap (`project/ScriptCommands.scala`). The one flag the
measurement does pass is `-no-specialization`, which is nsc's own — see below.

The build is a bootstrap (scala/scala builds itself with the *previous*
release's compiler), but that costs us nothing: we have real scalac 2.13.16, so
every dependency the library needs is already built.

## The numbers

`tests/scalalib_measure.sh`, on `agent/scalalib`:

| | files | errors | files with errors | classes |
|---|---|---|---|---|
| **`--no-scala-library`** (default) | 538 | **4203** | **219** | 0 |
| `SCALALIB_MODE=jar` (`--scala-library`) | 538 | 4383 | 221 | 0 |

`classes=0` is expected while errors remain — nothing is emitted — but read the
two together: `errors=0 classes=0` would mean a crash, not a success.

319 of the 538 files draw no diagnostic at all.

**These are the `agent/scalalib` numbers.** The next section, "The one root",
is the survey that was made of them; everything below "The `agent/preludeshadow`
slice" is what happened when that root was fixed. That slice took it to 1997
in 173, `agent/tuplelit` to the current default-mode figure of **1969 errors
in 172 files**.

### The measurement is not run against the jar

Linking `src/library` against `scala-library-2.13.16.jar` asks the compiler to
typecheck definitions of the classes that jar already contains, and every one
of them then reports a duplicate (`type mismatch; found: <overload None$ |
None$>`). The default mode is therefore `--no-scala-library` with only the
*Java* half of the library on the classpath, which is the arrangement that
would actually retire the jar. `SCALALIB_MODE=jar` measures the other one; the
two numbers are close because, as the next section shows, the duplication is
mostly against the built-in prelude rather than against the jar.

### Where it started, and why the number went up

The first measurement was **142 errors in 48 files** — and every one of them
was a *parse* error. As with cats and gitbucket, that number was hiding the
compiler: a file that does not parse is never typed, so 490 files were being
counted as clean without a single member ever being looked up. Getting the 538
files to parse took the number to 4203. **That is progress, not a regression.**

The parse failures were six things:

| Symptom | Sites | What it is |
|---|---|---|
| `unimplemented syntax: annotation specialized` / `unspecialized` | 84, in 40 files | `@specialized`; see below |
| `expected ), found dot` / `expected expression, found rparen` | 12 sites | `f(using x)` — a `using` argument clause |
| `expected expression, found comma` | 5 sites | `import a.b, c.d` — one clause, several importers |
| `expected ), found at` | 2 sites | `@(deprecated @companionMethod)(…)` — a parenthesised, meta-annotated annotation type |
| `unimplemented syntax: annotation elidable` | 4, all in `Predef.scala` | `@elidable(ASSERTION)` |
| `expected expression, found val` | 1, in `StringContext.scala` | a `${ … }` hole holding more than one statement |

All six are fixed on this branch; `tests/fixtures/scalalib_syntax.scala` and
`scalalib_spec.scala` pin them, and both dual-run against real scalac 2.13.16.

Two of them were not what they looked like:

* The `StringContext.scala` failure reads as "a triple-quoted string nested
  inside an interpolation hole", because that is the line it points at. It is
  not. The hole was parsed as a single *expression*: `s"""x ${ val a = 1; a }"""`
  fails with no nesting anywhere. nsc lexes `${` as an ordinary `LBRACE` and
  parses the hole with `blockExpr`; the lexer here consumed the brace pair
  without emitting tokens, so the hole could only ever hold one expression. The
  fix is in the lexer, not the parser, and it matters twice: emitting the brace
  pair also gives the line-break filter the right region, without which a hole
  written inside a `(…)` argument list inherits the paren region and loses the
  newlines separating its own statements.
* `-Xsource:3` has nothing to do with `f(using x)`. nsc 2.13.16 accepts a
  `using` argument clause with no flag at all (`Parsers.scala`: `case
  IDENTIFIER if in.name == nme.using && lookingAhead(isExprIntro)`), and passes
  the arguments positionally. `using` stays an ordinary identifier, so `f(using)`
  and `f(using.x)` still pass the *value* named `using`; the lookahead is what
  separates them.

### `@specialized`

The brief asked what to do about it, and noted that the parser rejected
`@specialized` **by name** while `import scala.{specialized => sp}; @sp` slipped
straight through. Both halves of that are now settled:

* nsc has a flag for exactly this — **`-no-specialization`**, "Ignore
  @specialize annotations" (`ScalaSettings.scala`). It is implemented here with
  that spelling. Under it `@specialized` and `@unspecialized` are dropped,
  which is what nsc does under it, so this is not a stub.
* **Without** the flag they used to stay a diagnostic. Since stage 1 of
  [`docs/specialization.md`](specialization.md) they are accepted and recorded
  on the type parameter's symbol instead, and this measurement reports the same
  number either way (**1644** with the flag and without it, where before the
  flagless run aborted at the first annotation and reported 84). The phase is
  still missing, so the classes we emit still lack the `$mc*$sp` members real
  scalac's callers link against; `tests/spec_classfiles.sh` is the ledger that
  measures that against scalac directly.
* The rename hole is closed. The parser remembers selectors that rename
  `scala.specialized` / `scala.annotation.unspecialized`, so `@sp` is read as
  `@specialized` (`tests/fixtures/scalalib_spec.scala`, `sp_alias.scala`).

`@elidable` is a different case and is now simply accepted. `@elidable(level)`
elides a call only when `level < -Xelide-below`, and nsc's default for that
setting is `elidable.MINIMUM` = `Int.MinValue`, which no level is below. There
is no `-Xelide-below` here, so ignoring the annotation is nsc's behaviour at
every setting we accept. Whoever adds `-Xelide-below` has to implement elision
in the same change.

`@(T @meta)` is accepted by dropping the meta-annotations. A meta-annotation
(`@getter`, `@setter`, `@companionMethod`, …) only says which of the members a
definition expands into should carry the annotation, and this subset does not
redirect annotations onto accessors or companion members. Both shapes the
library writes are inert under that treatment: ``@(`inline` @getter @setter)``
on a private var (we never inline) and `@(deprecated @companionMethod)` on
`Predef.any2stringadd`. It is still a fidelity gap, and it is the one thing in
this slice that is not exactly nsc.

## What the 4203 are

| Class | Count | Share |
|---|---|---|
| `X is not a member of Y` | 1391 | 33.1% |
| `type mismatch` | 1142 | 27.2% |
| `no matching overload` | 702 | 16.7% |
| overriding (`overrides nothing`, `override modifier required`, `incompatible type in overriding`, `cannot override final member`) | 263 | 6.3% |
| `needs to be abstract` | 261 | 6.2% |
| `not found: value` / `type` / `extractor` | 152 | 3.6% |
| `no matching overload for constructor` | 92 | 2.2% |
| `ambiguous overload` | 59 | 1.4% |
| `object creation impossible` | 32 | 0.8% |
| `type arguments do not conform` | 24 | 0.6% |
| `illegal inheritance` | 13 | 0.3% |
| everything else | 68 | 1.6% |

The receivers in `X is not a member of Y` say what is really going on:

```
184 IterableOnce   128 Iterable   97 Array   77 Int   50 Seq   49 Map
 46 IterableOps    37 List        36 String  36 (T1,…,T9)  25 IndexedSeq
```

Every one of those is a type the prelude builds.

## The one root

**A source definition of a name the prelude also supplies is invisible: the
prelude's symbol wins, and the source members are not found.** Nine lines:

```scala
package scala.collection

trait IterableOnce[+A] {
  def myOwnMember: Int
}

object P {
  def f(it: IterableOnce[Int]): Int = it.myOwnMember
}
```

```
error: value myOwnMember is not a member of IterableOnce[Int]
```

That is the whole of `IterableOnce.scala`'s 123 errors (`value iterator is not
a member of IterableOnce[A]`, on the file that *defines* `iterator` twenty
lines above), and it cascades outward: `Iterable` cannot find its parents'
members, so `Seq`, `Map`, `List`, `Vector` and everything typed through them
follow. The 335 `value iterator is not a member of …` are one bug, not 335.

The same collision has a second face where a *value* is involved: the source
and prelude symbols end up in one overload set, and

```
type mismatch; found: <overload None$ | None$>  required: Option[ProcessLogger]
value :: is not a member of <overload Nil$ | <overload Nil$ | Nil$>>
```

is what `scala/sys/process` reports. Note this is `--no-scala-library` mode:
the duplicate is against the *prelude*, not against the jar.

Checked against the jar, the constructs themselves are fine. Member lookup
through a higher-kinded parameter's bound, which produces 116 errors in
`LazyZipOps.scala` and 95 in `Equiv.scala` when the library is compiled from
source —

```scala
final class IterableEquiv[CC[X] <: Iterable[X], T] {
  def equiv(x: CC[T]): Boolean = x.iterator.hasNext
}
```

— compiles cleanly with `--scala-library`. It is the source `Iterable` that is
not reachable, not the bound.

## The `agent/preludeshadow` slice: source definitions now replace the prelude

**4180 errors in 218 files → 1997 in 173**, measured on this branch merged
with `main` at `d4131b0`. Every other target is unchanged to the error; see
the table at the end of this section.

The root above was right, and it was three separate mechanisms wearing one
symptom. All three are name resolution; none of them is in `prelude*.rs`
itself, which turned out not to need editing at all.

### 1. A source definition replaces the prelude's symbol

`SymbolTable::shadow_supplied_by_source`, called from the namer as each
source class, object and synthesized companion is entered. When the owner
already holds a prelude symbol (`id < prelude_end`) of the same name in the
same namespace, that symbol is made **unreachable by name**:

* out of the owner's `members`, which is what `lookup_member` walks and what
  the per-`PackageDef` scope is built from — entering the source symbol
  *alongside* the prelude's is not enough, because the prelude's was
  allocated first and every "first class-like hit" therefore found it;
* out of **every open scope**, replaced in place by the source symbol. This
  is the half that is easy to miss: the prelude enters names like
  `IterableOnce`, `<:<` and `Ordered` into a scope of its own by hand, and
  that scope never pops, so an entry there outlives any shadowing a package
  scope could do;
* skipped by `find_class_by_jvm`, which resolves a binary name to the
  *lowest* symbol id carrying it — always the prelude's.

The symbol itself stays in the table. Its id is already written into prelude
signatures (`Map.get` returns the prelude's `Option`, and so on) and a namer
cannot rewrite those. Only prelude symbols are replaced; a classfile on the
classpath is a real ambiguity that nsc reports, and is left alone.

### 2. `scala._` was a snapshot, not an import

nsc opens `java.lang._`, `scala._` and `Predef._` around every unit. The
prelude models the `scala._` half by *copying* the package's members into its
scope at install time, which a source `Tuple9.scala` compiled in the same run
arrives too late for. In `--no-scala-library` mode the prelude builds no
`Tuple1` and no `Tuple3`…`Tuple22` at all — the private runtime has no such
classes — so the source ones were the only ones in existence and were
invisible from any other package: `class_sym_of` answered `None` for
`(T1, …, T9)`. `Typer::auto_import_scala_member` enters a source definition
whose owner is package `scala` into the prelude's scope as well.

### 3. `TupleN` was looked up in the wrong namespace

`class_sym_of(Type::Tuple(..))` used the namespace-blind `SymbolTable::lookup`,
which stops at the nearest scope that binds the name *at all*. `object Equiv`
and `object Ordering` each declare `implicit def Tuple2[T1, T2](…)`, so inside
their bodies the lookup stopped at the method, found nothing class-like in it,
and gave up — **176 errors**, all of them `value _1 is not a member of
(T1, T2)` in the two files that define the tuple orderings. `lookup_type` is
the right lookup; it skips a scope that binds the name only as a term.

### What this did to the numbers

Each of the three is worth its own line; these were taken at the branch point
`56174d5`, one after the other:

| | files | errors | files with errors | classes |
|---|---|---|---|---|
| branch point (`56174d5`) | 538 | 4203 | 219 | 0 |
| after (1) | 538 | 2245 | 176 | 0 |
| after (2) | 538 | 2190 | 172 | 0 |
| after (3) | 538 | **2014** | **172** | 0 |

And on the merged tree, which is the number that counts:

| | files | errors | files with errors | classes |
|---|---|---|---|---|
| `main` at `d4131b0` | 538 | 4180 | 218 | 0 |
| this branch merged with it | 538 | **1997** | **173** | 0 |

`classes=0` is still expected while errors remain.

### One thing that looked right and was not

Making `tree_to_type`'s hand-written shortcuts for `Option`, `List` and `Some`
— which map an applied type tree with that *name* straight onto
`st.option_sym` / `list_sym` / `some_sym`, whatever prefix it is written with
— yield to a source definition of `scala.Option` takes the measurement from
**2014 errors in 172 files to 2251 in 205**. The prelude's `Option` carries
members the source one has no working signature for yet, so redirecting the
name loses more than it gains. It is left as it is, with a comment saying so.

Those shortcuts have a second, independent consequence, unrelated to
`src/library` and present on the branch point: `mine.Option[Int]`, written
with an explicit unrelated prefix, also comes out as `scala.Option[Int]`.
`crates/cli/tests/preludeshadow.rs` records it.

### The other targets, before and after this slice

Both columns measured on the merged tree, the "before" one from a binary built
with `main`'s (`d4131b0`) `check.rs` / `symbol.rs` / `prelude.rs` in place.

| | before | after |
|---|---|---|
| `tests/slick_measure.sh` | `files=184 errors=0 files_with_errors=0 classes=1596` | identical |
| `tests/slick_run.sh` | — | `progs=12 ok=12 diff=0 fail=0` |
| `tests/cats_measure.sh` | `files=339 skipped=1 errors=71 files_with_errors=16 classes=0` | identical |
| `tests/gitbucket_measure.sh` | `files=353 skipped=1 errors=1859 files_with_errors=186 classes=0` | identical |
| `tests/scala_corpus.sh` (sample, 250/kind, `CORPUS_JOBS=6`) | `pos 134/250 · neg 101/250 · run 49/250` | identical, and the per-test TSVs `diff` clean |
| `cargo test --workspace --release` | — | 146 × `test result: ok`, 1931 tests |

`tests/slick_subset.sh` was not run: this slice touches no code generation
(`crates/backend/` is untouched), so its 30 minutes would measure nothing.

Note that `docs/cats.md`'s headline number (3019) and `docs/gitbucket.md`'s
(2545) are both stale: on `d4131b0` cats measures **71** and gitbucket
**1859**.

## The `agent/tuplelit` slice: a tuple literal is `scala.TupleN`

**1997 errors in 173 files → 1969 in 172**, measured on this branch merged with
`main` at `0b57b6a` against that `main` itself. Every other target is unchanged
to the error; see the table at the end of this section.

This was the first item on the previous slice's "what to do next" list, and
that entry was right about the mechanism and **wrong about the size** — see
"How big it was".

### The mechanism

The parser lowers `(a, b)` to `Apply(Ident("Tuple2"), …)`, and the typer then
resolved that `Ident` like any other name, so a *term* of that name in a nearer
scope answered for it. `scala.math.Ordering` and `scala.math.Equiv` each
declare `implicit def Tuple2[T1, T2](…)` and write tuple literals in their own
bodies. Real scalac 2.13.16 accepts

```scala
object Fake {
  def Tuple2(n: Int): String = "" + n
  def f[A, B](a: A, b: B): (A, B) = (a, b)
  def g(x: Int, y: Int): Int = (x, y) match { case (a, b) => a + b }
}
```

because `gen.mkTuple` builds a fully qualified `scala.TupleN` tree: the name is
never looked up.

### The design decision

The brief said `Tree` has no marker field. It has one — `postfix`, set the same
way, by mutating the node after `alloc` — and the cheapest place to put a
second one is beside it. `Tree` gains `scala_ref`: *"an `Ident` the compiler
made up for a name nsc writes fully qualified."* Adding a field to `Tree` costs
100 mechanical edits at its struct literals; adding one to the `Ident` variant
would have cost 218 pattern edits, and rewriting the synthesis to
`Select(Ident("scala"), "TupleN")` would have broken the several places that
match `Apply { fun: Ident { name } }` against `Tuple{n}`.

`scala_ref` is set in four places: the parser's tuple *expression* and tuple
*pattern*, and the two `Ident("TupleN")` trees `check.rs` synthesizes itself
(auto-tupling an argument list, and a `for` generator's destructuring
selector). It is read in two: `Typer::type_ident`, and the constructor-pattern
arm of `type_pattern`, which resolves the pattern's class separately and needed
the same treatment — without it `case (a, b) =>` still reported `not found:
extractor Tuple2`.

Resolution is `SymbolTable::lookup_scala`: a member lookup in package `scala`,
with a class/module-only lexical fall-back for `--no-scala-library` mode, where
the prelude enters some names into a scope of its own rather than into the
package. It never sees a term, so nothing can capture it.

Everything the *source* writes keeps ordinary resolution, which is the half
that is easy to get wrong and impossible to see in a diagnostic count: an
explicit `Tuple2(1)` must still call the method in scope, as it does under
scalac. `tests/fixtures/tuplelit_shadow.scala` is run, not just compiled, for
exactly that reason, and dual-run against real scalac 2.13.16.

### The rest of the parser's synthesized names

The brief asked whether other synthesized `Ident`s have the same hole. The
parser makes up `Function0`/`FunctionN`, `Unit`, `Throwable`, `<repeated>`,
`<tuple>`, `_root_`, `<empty>` and `x$pf`; `check.rs` adds `StringContext` and
`Tuple2`. All but two were already safe — the type-position ones go through
`lookup_type`, which `agent/preludeshadow` fixed, and the rest are names the
surface syntax cannot bind.

* **`StringContext` is not in this family, and must not be.** nsc really does
  emit an unqualified `StringContext`, and scalac 2.13.16 reports `value s is
  not a member of String` for an `s"…"` written where a `def StringContext` is
  in scope. We *accept* that program, which is a fidelity gap in the opposite
  direction; qualifying the name would have written the gap into the compiler
  deliberately.
* **One more hole, found the same way.** `Typer::seq_of`, which widens a
  repeated parameter `T*` to `Seq[T]`, used `SymbolTable::lookup`, so a
  `def Seq` in scope left the parameter as the bare `T*` and every selection on
  it failed (`value length is not a member of Int*`). It now uses
  `lookup_type`, the same fix `class_sym_of` got. It is worth no errors in
  `src/library` — nothing there shadows `Seq` — but it is the same bug and
  reproduces in four lines.

`Array(1, 2)` next to a `def Array(n: Int)` is *not* a bug: scalac rejects it
too (`too many arguments … for method Array`). A rejection rule that fires
there would have been wrong.

### How big it was

The entry above predicted "most of the 288 `no matching overload`". It was 2 of
them. The 28 errors this removed are mostly `value apply is not a member of
TupleN`, `value _1 is not a member of …` and the type mismatches cascading from
those. Six files improved and none regressed:

| file | before | after |
|---|---|---|
| `scala/math/Equiv.scala` | 3 | **0** |
| `scala/collection/mutable/CollisionProofHashMap.scala` | 23 | 13 |
| `scala/sys/process/ProcessImpl.scala` | 11 | 7 |
| `scala/collection/LazyZipOps.scala` | 22 | 18 |
| `scala/collection/immutable/NumericRange.scala` | 14 | 10 |
| `scala/math/Ordering.scala` | 10 | 7 |

`Ordering.scala`'s remaining 7 are unrelated: `override def max[U <: T](x: U,
y: U): U` against the same signature in the parent reports `incompatible type
in overriding`, which is polymorphic override checking, not lookup.

### The other targets, before and after this slice

`main` moved twice during the slice and was merged in both times, so these are
**the merged tree against `main` at `0b57b6a`**, measured back to back on the
same machine — not against the branch point.

| | `main` at `0b57b6a` | merged with this branch |
|---|---|---|
| `tests/scalalib_measure.sh -no-specialization` | `files=538 errors=1997 files_with_errors=173 classes=0` | `files=538 errors=1969 files_with_errors=172 classes=0` |
| `tests/slick_measure.sh` | `files=184 errors=0 files_with_errors=0 classes=1596` | identical |
| `tests/slick_run.sh` | `progs=12 ok=12 diff=0 fail=0` | identical |
| `tests/cats_measure.sh` | `files=339 skipped=1 errors=2929 files_with_errors=151 classes=0` | identical |
| `tests/gitbucket_measure.sh` | `files=353 skipped=1 errors=1693 files_with_errors=186 classes=0` | identical |
| `tests/scala_corpus.sh` (`CORPUS_SIZE=full`, `CORPUS_JOBS=6`) | `pos 977 · neg 640 · run 443` | identical, and the two `corpus.tsv` files `diff` clean — byte for byte, recorded diagnostic included |
| `cargo test --workspace --release` | — | 150 × `test result: ok`, 1970 tests, 0 failures |

`main` then moved again, to `cad281b` (`agent/kindproj`), which touches
`crates/typer/src/symbol.rs` and `crates/parser/src/parse.rs` — both files this
slice edits. The merge was clean, and the whole set was re-run on it:
scalalib `1969 / 172`, slick `errors=0 classes=1596`, `slick_run` 12/12, cats
`2929`, gitbucket `1693`, corpus `pos 977 · neg 640 · run 443`, workspace
151 × `test result: ok` / 1976 tests / 0 failures. Every "after" figure above
is unchanged by that merge.

`tests/slick_subset.sh` was not run: this slice touches no code generation
(`crates/backend/` is untouched), so its 30 minutes would measure nothing.

Two numbers in the brief this slice was handed did not reproduce here, and
neither is anything to do with the change: corpus `run` is **443**, not 434,
and gitbucket is **1693**, not 1859 — the latter because `main` improved it in
the meantime. A per-test `diff` of the corpus TSVs is the check worth running
either way; a `run` total on its own moves with the per-test timeouts and with
whatever else the machine is doing.

Which of the checks in `.agent-brief.md` this slice actually ran: the four
measurement scripts above (compile only, `classes=0` on scalalib/cats/gitbucket
is expected while errors remain), `tests/slick_run.sh` and `tests/conform/`
(the two that execute code), the corpus, and the full workspace suite. The
classfile-loader and `javap` sweeps in `slick_subset.sh` were skipped as above.

## A constructor alias that ignores its parameter (`agent/anyconstr`)

**1420 -> 1367 errors**, 53 gone and none new. Measured on `fce0a0d8` and
again after merging `main` at `f4b829ec` (`agent/gbopt`) — the same 53 error
locations gone each time and none appeared. Both A/Bs compare two saved
binaries (`SCALA_RS=<binary>`), never a revert in the working tree.

`scala.collection` declares `type AnyConstr[X] = Any`. It is a type
*constructor* whose body does not mention its parameter, so **every**
one-parameter constructor conforms to it, and `IndexedSeq#stepper`'s

```scala
case _ => shape.parUnbox(new AnyIndexedSeqStepper[A](this, 0, length))
```

really does pass an `IndexedSeqOps[A, CC, C]` where an `IndexedSeqOps[A,
AnyConstr, _]` is wanted. All 53 were that one shape, in five files:
`Iterable.scala` (25), `IndexedSeq.scala` (11), `Seq.scala` (9), `Set.scala`
(3) and the rest of `collection`.

### The rule, not the instance

`type F[X] = Any` is the degenerate case; the general question is how nsc
compares two type constructors of the same arity when one of them is an alias,
and the answer is `normalize` plus `sameLength` in `Types.scala`. `isHKSubType`
normalizes both sides — which eta-expands each `TypeRef` to a `PolyType` and
beta-reduces an alias body — and `isPolySubType` then requires `sameLength` on
the parameters and compares the bodies. `[x]CC[x] <:< [X]Any` holds for every
`CC` because `CC[x] <:< Any` does; `[x]Option[x] <:< [X]List[X]` does not,
because `Option[x] <:< List[x]` does not. One rule decides both.

This compiler already compared two type constructors that way —
`SymbolTable::eta_expand_pair`, built by `agent/hkunify`, where a `Type::Class`
with fewer arguments than the class has parameters *is* the curried
constructor. What it declined was the pair where one side is an **abstract**
constructor: a higher-kinded type parameter (`CC[_]`) or an abstract type
member. That is exactly the library's case, and it is the only thing this slice
adds (`eta_expand_abstract_pair`). The kinds have to agree as well as their
number, which is nsc's `corresponds`/`cmp` beside its `sameLength`.

The new arm is deliberately **widening-only**. Where both sides eta-expand,
their bodies are the whole answer and a `false` is an answer; an abstract
constructor's body is `CC[x]` and settles nothing on its own, so a `false`
there still falls through to the bound, path-member and projection arms below
it. Reducing more eagerly than that is what would make unrelated constructors
conform.

### The overload it was silently getting wrong

`tests/fixtures/ac_anyconstr.scala` runs and prints the name of the method body
that ran, so an acceptance that answered the conformance question the wrong way
cannot pass. Three of its calls did not compile before the fix. The fourth did
— and printed the wrong answer:

```scala
def sel(x: Ops[Int, AnyConstr, _]): String = "sel:anyconstr"
def sel(x: Any): String                    = "sel:any"
```

With an abstract `CC`, `sel(this)` fell through to the `Any` overload and
compiled. `expected/ac_anyconstr.txt` is real scalac 2.13.16's own run of the
same source, and the e2e test asserts scalac's run against it as well as ours.

`ac_anyconstr_bad.scala` is the restriction: `type G[X] = List[X]` must not
make `Option` conform to `G`, an abstract `CC` is not `List` either, and the
reduction is not symmetric (`Ops[Int, AnyConstr, String]` is not an
`Ops[Int, CC, String]`). Real scalac rejects all three at lines 26, 30 and 35
and so do we, at the same lines. They were rejected before the fix too — the
negative fixture is what stops the new arm from over-reaching, not evidence
that it exists.

### One divergence found and left alone

nsc treats `Any` and `Nothing` as **kind-overloaded**: `checkKindBoundsHK`
skips the arity check when the argument's `typeSymbol` is `AnyClass` or
`NothingClass`, so real scalac accepts `Ops[Int, Two, _]` for
`type Two[X, Y] = Any` against a `CC[_]` parameter, where we report `kinds of
the type arguments (Two) do not conform`. It is a separate defect in kind
checking, it costs the library nothing (`AnyConstr` is arity 1 and used at
arity 1), and it is not touched here.

## The `agent/libanyval` slice: an overload is not an override

**1173 errors in 157 files → 1139 in 155**, measured on `agent/libanyval`
merged with `main` at `b15df464` (`agent/anyconstr`, `agent/libmaxmin`). The
same 34 go on the branch's own base, `fce0a0d8`, where they read
**1420 / 166 → 1386 / 164**: they are exactly the two override clusters —
22 `` `override` modifier required to override concrete member`` and
12 `cannot override final member`` — and **no new error appeared in any
cluster**, before or after the merge. Every other target is unchanged; see the
table at the end of this section.

### The question this slice was set, and the answer

The slice was asked to decide, *with evidence*, between two readings of those
34 errors:

1. nsc never compiles `Any.scala` / `AnyVal.scala` / `Int.scala` at all — they
   are scaladoc stubs — so the honest fix is to hold them out of the measure
   the way `gitbucket_measure.sh` holds out `PullRequestsController.scala`;
2. nsc really does compile them, and our override rules are wrong.

**It is (2), and the framing of the question was itself off by an order of
magnitude.**

* `Any.scala`, `AnyRef.scala`, `Nothing.scala`, `Null.scala` and
  `Singleton.scala` live in **`src/library-aux`**, which `scalalib_measure.sh`
  already does not compile (scala/scala's `build.sbt` passes that directory as
  `-doc-no-compile`). Nothing to hold out: it was never in.
* `AnyVal.scala` and the nine value classes live in **`src/library`** and nsc
  compiles them. `Definitions.scala` proves it —
  `lazy val AnyValClass: ClassSymbol = (ScalaPackageClass.info member tpnme.AnyVal orElse {…})`
  *reads `AnyVal` out of the compilation unit* when there is one.
* And only **one** of the 34 was about the top of the hierarchy at all. The
  other 33 were `Map`, `SortedMap`, `IntMap`, `LongMap`, `AnyRefMap`,
  `ArrayBuffer`, `ListBuffer`, `ArrayDeque`, `UnrolledBuffer`,
  `JavaCollectionWrappers`, `PartialFunction`, `Stepper`, `ArrayBuilder`,
  `CollisionProofHashMap` and `typeConstraints` — nothing to do with `Any`.

So nothing was held out, and the number below is a real reduction.

### The 33: parameter types are invariant, and we could not tell

`override_check::same_type` gave up and answered **"same type"** whenever
either side mentioned a type parameter (`robust` refuses to compare those, and
the comparison defaulted to matching). That is the right default for a *result*
comparison, where a wrong answer invents a diagnostic; it is the wrong default
for deciding **whether two methods are the same method at all**, because SLS
5.1.4 makes parameter types invariant under overriding and a different one is
an overload.

The library is written in exactly the shape that breaks:

| declaration | its supposed override | why they are two methods |
|---|---|---|
| `IterableOps.concat[B >: A](suffix: IterableOnce[B])` | `MapOps.concat[V2 >: V](suffix: IterableOnce[(K, V2)])` | read at `Map`, `IterableOnce[V2]` against `IterableOnce[(K, V2)]` — a bound variable is not a tuple containing it |
| `Buffer.prepend(elems: A*)` (`final`) | `ArrayBuffer.prepend(elem: A)` | `scala.<repeated>[A]` is not `A` |
| `Function1.compose[A](g: A => T1)` | `<:<.compose[C](r: C <:< From)` | `<:<` is a *subtype* of `Function1`, not the same type |
| `Growable.addAll(elems: IterableOnce[A])` | `ArrayBuilder.addAll(xs: Array[_ <: T])` | unrelated classes |
| `Spliterator.OfInt.tryAdvance(Consumer[_ >: Integer])` | `…tryAdvance(IntConsumer)` | unrelated classes |

`certainly_different` now decides these, and only these — it is the
counterweight to `robust`, and it answers `false` (undecided) for everything it
cannot settle by *shape*: repeated against non-repeated, by-name against
non-by-name, the same head class with a decidably different argument, two
classes with different JVM names, and a **rigid** type variable against an
application of a constructor. "Rigid" is guarded by `rigid_tparams`: only the
overriding method's own parameters and those of the class the check runs in
count, because a leftover parameter is a `subst_as_seen_from` that failed and
nothing may be concluded from it.

**Three things it deliberately does *not* decide**, each of them a regression
the cats and gitbucket measurements caught before this landed:

* **Arity, of a function or a tuple.** `Function0` really is not `Function1`,
  but the arity scala-rs *stores* is not always the arity the source wrote:
  `Unit => X` arrives as `Function { params: [], ret: X }`. Ten
  `f: Unit => F[A]` parameters in cats (`OptionT`, `EitherT`, `IorT`) read as a
  different arity from the `E => F[A]` they override — **cats 188 -> 197**,
  nine `overrides nothing` errors scalac does not report.
* **A head spelled out rather than looked up.** An early version compared
  `"scala/Array"` against a symbol's `jvm_name`; they disagree, and
  `TC[C[_]] { def sizeOf(c: C[Int]) }` implemented at `C = Array` stopped
  counting as an implementation (`tests/fixtures/at.scala`). `head_sym` asks
  the symbol table and nothing else.
* **One reading of the child's signature.** nsc's `matchingSymbols` reads both
  sides at the same prefix, and doing that fixed four spurious
  `needs to be abstract` errors in the library — but `subst_as_seen_from` is an
  approximation, and reading the child through it is not always an improvement:
  gitbucket's fifteen controllers implement scalatra's
  `Initializable.initialize(config: ConfigT)` from a class file, and read at the
  controller the implementation stopped matching — **gitbucket 270 -> 285**.
  `matches` now takes the *union* of the two readings, and
  `provably_overloaded` the intersection. Both land on the side this module has
  always been on: silence when a comparison is not to be trusted.

Two supporting corrections came out of the same measurement:

* **`matches` now reads both signatures at the same prefix**, as nsc's
  `matchingSymbols` does (`self.memberType(sym1)` against
  `self.memberType(sym2)`). The child was being read raw. That is invisible
  while the child is owned by the class, but
  `check_missing_implementations` asks whether a member inherited from *one*
  base implements a declaration inherited from *another*, and it was comparing
  `Iterator.filter(p: A => Boolean)` with
  `IterableOnceOps.filter(pred: Seq[B] => Boolean)`. Without this, tightening
  the matcher produced four new `needs to be abstract` / `object creation
  impossible` errors.
* **The backend must not bridge an overload.** `bridge_overrides` compares
  *erased* descriptors on purpose — that is what a bridge exists for — so once
  `Table.pp[V2](xs: Bag[(K, V2)]): String` was correctly accepted beside
  `Ops.pp[B](xs: Bag[B]): Bag[B]`, both bridge emitters saw one `(LBag;)`
  parameter and emitted `pp(LBag;)LBag;` calling `pp(LBag;)Ljava/lang/String;`
  with a `checkcast`. The class then threw `ClassCastException` on the
  parent's own signature where scalac emits a mixin forwarder. The relation
  cannot be recomputed in the backend — by then `Bag[B]` and `Bag[(K, V2)]`
  are both just `Bag` — so `record_method_override_families` now freezes the
  *complement* too, in `SymbolTable::method_overload_pairs`, and
  `method_overloads` is what the emitters ask.

### The 1: `Object` is not a base class of `AnyVal`

```
error: `override` modifier required to override concrete member:
def getClass: Class[_] (defined in class Object)
 --> src/library/scala/AnyVal.scala:57:3
 57 |   def getClass(): Class[_ <: AnyVal] = null
```

nsc's `AnyClass` is `enterNewClass(ScalaPackageClass, tpnme.Any, Nil, ABSTRACT)`
— **`Nil` parents** — and `AnyVal extends Any`, so `java.lang.Object` is not a
base class of `AnyVal`, of the nine primitive value classes, or of a
user-defined value class. `override_check::full_lin` appended `Object` and
`AnyRef` to *everything*. It no longer does for an `AnyVal`-rooted template.

The boundary is nsc's own, and it is narrower than "extends `Any`".
`RefChecks.checkAllOverrides` carries a second, JVM-motivated ban whose guard
is `clazz.isTrait && !clazz.isSubClass(AnyValClass)`:

```
scala> trait Univ extends Any { def notify(): String = "x" }
error: trait cannot redefine final method from class AnyRef
scala> class Meters(val n: Int) extends AnyVal { def notify(): String = "x" }   // accepted, and runs
```

So a **universal trait** keeps `Object` in its linearization here — that is
what preserves the rejection, under the ordinary rule's wording — and only
`AnyVal` and its subclasses are exempt. `is_any_rooted` recognises the
source-compiled `abstract class AnyVal extends Any` structurally: it is the
only *class* whose parents are exactly `Any`, because scalac rejects any other
with `Any does not have a constructor`.

### Fixtures

`tests/fixtures/libanyval_overload.scala` executes five overload pairs and a
value class; `tests/fixtures/expected/libanyval_overload.txt` is real scalac
2.13.16's own output for it. Three negative fixtures pin the boundary —
`libanyval_final_bad` (a base type parameter the subclass *fixes* still
overrides a `final` member), `libanyval_modreq_bad` (`A*` against `Int*` is not
a shape difference), `libanyval_univtrait_bad` (a universal trait still may not
redefine `Object`'s finals). All four are in `crates/cli/tests/override.rs`.

### The other targets, before and after this slice

Measured on this branch merged with `main` at `b15df464`; "before" is that
merge base, built and measured in the same tree rather than read off a table.
(The cats and gitbucket regressions in the section above were found exactly
this way, and both are back at the base figure.)

| target | before (`b15df464`) | after |
|---|---|---|
| scala library | `1173 / 157` | **`1139 / 155`** |
| gitbucket | `270 / 79` | `270 / 79` |
| cats | `185 / 71` | `185 / 71` |
| slick (compile) | `errors=0 classes=1490` | `errors=0 classes=1490` |
| slick (`MODE=b`) | `progs=12 ok=12 diff=0 fail=0` | `progs=12 ok=12 diff=0 fail=0` |

The head of what remains, re-clustered on the 1139: 22 `class TupleN needs to
be abstract`, 18 `value + is not a member of <notype>`, 15
`found: <overload Stream[A] | Iterable[A] | Stream[A]> required: Stream[A]`,
13 `found: T required: A`, 13 `no matching overload for
(MainNode[K, V], …)Boolean`, 11 `value min is not a member of <notype>`, 9
`incompatible type in overriding`, 9 `value & is not a member of Boolean`. The
overriding family is now 9 `incompatible type in overriding` plus 10
`overrides nothing`, and both of those are the member-lookup root seen from
the other side.

### A defect found and not fixed here

A subclass method shadows a same-named **inherited** method entirely when the
parent is generic, so a legal call to the inherited overload is rejected:

```scala
trait Bag[+A] { def tag: String }
trait Ops3[A] { def h(x: Bag[A]): String = "Ops3.h" }
class T3 extends Ops3[Int] { def h(x: Int): String = "T3.h" }
new T3().h(new Bag[Int] { def tag = "b" })   // no matching overload for (Int)String
```

This predates the slice — `Bag[Int]` and `Int` are both fully resolved, so the
old matcher already treated them as an overload — and it is member-supply, not
override checking. It is why `libanyval_overload.scala` upcasts before calling
the inherited alternative.

## What to do next, in order

Re-clustered on the 1367 that remain (`agent/anyconstr`, merged at `515a43c4`): 474
`type mismatch`, 453 `X is not a member of Y`, 157 `no matching overload`, 42
`no matching overload for constructor`, 33 `not found: value`, 32 `needs to be
abstract`, 22 `` `override` modifier required``, 20 `ambiguous overload`.
(`agent/libanyval`, below, has since removed all 22 of the
`` `override` modifier required`` and the 12 `cannot override final member`.)

1. **The prelude collision is still the whole first half.** The receivers in
   `is not a member of` are `Int` (69), `Array` (68), `<notype>` (51) and
   `String` (36); the `<overload Stream[A] | Iterable[A] | Stream[A]>` spelling
   accounts for 38 more between the two classes. This is the root named above
   under "The one root" and nothing in it has changed.
2. **`new Array(WIDTH)` does not take its element type from the expected
   type** — `a2 = new Array(WIDTH)` where `a2: Array[Array[AnyRef]]` reports
   `found: Array[Nothing]`. About 28 errors at five nesting depths, nearly all
   in `Vector.scala`, which is the largest single mechanism outside the
   collision.
3. **`Iterator.empty.next()` leaves the element parameter uninstantiated** —
   13 × `type mismatch; found: T  required: A`, in `IndexedSeqView.scala` and
   `Iterator.scala`. `Iterator.empty` is an `Iterator[Nothing]`, so `next(): T`
   is `Nothing` and conforms to every `A`.
4. Do **not** assume the overriding family (now 54) is a second root. The ones
   sampled were the same collision seen from the other side, and it has shrunk
   in step with everything else, which is consistent with that reading.
5. `src/reflect` and `src/compiler` are not worth measuring yet.

## The `agent/libmaxmin` slice: `Predef._` is an import, not a snapshot

**1420 errors in 166 files → 1226 in 157**, measured on this branch merged with
`main` at `f4b829ec` (`agent/gbopt`). The brief handed this slice 55 errors —
34 `value max is not a member of Int` and 21 `value min` — together with a
hypothesis about the pickle seam. The hypothesis was wrong in both halves, and
saying how is most of the value of the entry.

### What the brief said, and what is actually true

> `Predef`, `RichInt` and `intWrapper` are being *defined* by the files under
> compilation while also being supplied by the `--scala-library` jar the
> measure links against, and something about that seam loses the conversion.

Three measurements say otherwise.

* **The measure does not link the jar.** `tests/scalalib_measure.sh` runs
  `--no-scala-library` by default — the section "The measurement is not run
  against the jar" above says so — and in that mode `prelude.rs` gates *both*
  `RichInt` and `intWrapper` on `library_abi`, so neither is built at all.
  There was no jar copy to win the seam, because there was no second copy.
* **The source declaration is not shadowed and is not un-implicit.** Writing
  `import scala.Predef._` by hand in the same file makes both `a xmax b` and an
  explicit `scala.Predef.xintWrapper(a).xmax(b)` resolve. The conversion was
  simply not in scope.
* **It has nothing to do with `src/library`.** It reproduces in fifteen lines
  with no library source anywhere (`crates/cli/tests/libmaxmin.rs`,
  `source_predef_conversion_is_in_scope_everywhere`).

### The cause

nsc opens `java.lang._`, `scala._` and `Predef._` around every unit, and
`Predef` there means whatever `scala.Predef` resolves to. This compiler
modelled the third by *copying* the prelude's `Predef` members into the base
scope at install time (`prelude::import_members`), before any source is read.
A run whose own sources define `scala.Predef` therefore got a scope describing
a `Predef` the program does not have.

This is the same defect the `agent/preludeshadow` section records for the
`scala._` half — "`scala._` was a snapshot, not an import" — and that slice's
`Typer::auto_import_scala_member` fixed only that half: it enters a source
*class or object* landing directly in package `scala`, and says nothing about
the *members* of a source `Predef`, which is a different scope.
`crates/typer/src/predef_reimport.rs` is the missing half. It runs from
`check::typecheck_units` between the signature pass and the body pass — the
members of a source `object Predef` do not exist before the signature pass, so
this cannot be done in the namer the way `auto_import_scala_member` is.

### Three things it has to get right

Each was measured by getting it wrong first.

| what | getting it wrong cost |
|---|---|
| **Replace, do not join.** A source member supersedes the prelude's snapshot of that name. | Entering them alongside made every `int2Integer` two candidates: **16** new `ambiguous implicit: X, X`, in a run that had had 2 ambiguities in total. |
| **Do not dedupe by name.** `Predef` overloads `require` and `assert`. | Keeping only the first binding cost **17** errors reading `no matching overload for (Boolean)Unit with arguments (Boolean, String)`, across nine files — every two-argument `require`/`assert` in the library. |
| **Record the import, not only its members.** | An inherited conversion's owner is a plain class, so codegen emitted the call on `this`: `3 bigger 7` type-checked and then died with `class Main$ cannot be cast to class scala.LowPriorityProbe`. `Typer::wildcard_module_for` recovers the receiver from a recorded wildcard — against the module **class**, because the module *value* carries no `parents` and `inherits_from` walks those. |

The third is why `tests/fixtures/libmaxmin_predef.scala` is **run**, not merely
compiled. Nothing else in this repository would have caught it: the JVM
verifier does not object, and every compile-only measure was green.

### What moved

52 files improved and 3 regressed:

| file | before | after |
|---|---|---|
| `scala/collection/immutable/ArraySeq.scala` | 32 | **5** |
| `scala/collection/Iterator.scala` | 34 | 15 |
| `scala/collection/View.scala` | 20 | 4 |
| `scala/collection/concurrent/TrieMap.scala` | 66 | 51 |
| `scala/collection/LazyZipOps.scala` | 12 | **0** |
| `scala/math/BigDecimal.scala` | 19 | 9 |
| `scala/collection/StringOps.scala` | 20 | 10 |
| `scala/math/BigInt.scala` | 4 | **0** |
| `scala/annotation/elidable.scala` | 2 | **13** |
| `scala/concurrent/Future.scala` | 14 | **23** |
| `scala/concurrent/duration/Duration.scala` | 18 | **22** |

The three regressions are one root and **28 errors of `value -> is not a
member`**, at sites that previously died one line earlier on `not found: value
Map`. It is a **pre-existing defect, not this slice's**: two conversions in
scope offering `->` from the same source type select neither, and it
reproduces with no `Predef` and no re-import anywhere —

```scala
package scala
object PredefY {
  implicit final class ArrowAssocQ[A](private val self: A) extends AnyVal {
    def -> [B](y: B): (A, B) = (self, y)
  }
}
// another file
import scala.PredefY._
object U { val a = 1 -> 2 }   // value -> is not a member of 1
```

Removing the prelude's competing conversion by name does not reach it: the
prelude spells it **`any2ArrowAssoc`** while the library's implicit class
synthesizes **`ArrowAssoc`**, so the names never meet. A rule keyed on
`prelude_shadowed` instead does not reach it either — the source `ArrowAssoc`
is owned by `object Predef`, not by package `scala`, so
`shadow_supplied_by_source` never records a victim for it. Both were tried and
measured at zero; neither is in the change.

### The other targets, before and after

See the slice's report for the full table; every measure other than the
library is unchanged to the error.

## The `agent/libprelude` slice: members the prelude does not declare

**1111 errors in 155 files → 1065 in 154**, on this branch merged with `main`.
The brief handed this slice one symptom — `value & is not a member of Boolean`
— and asked for an audit rather than a patch. Three things came out of it, and
the audit is worth more than any of them.

### The audit

`javap -p` over `scala-library-2.13.16.jar` lists every instance member of
`scala.{Boolean,Byte,Short,Char,Int,Long,Float,Double,Unit}` — **727** of them,
counting each overload. A generated probe puts one call to each through both
compilers, twice, because a *missing* member and a *wrongly typed* one fail
differently:

| probe | each call ascribed to | catches |
|---|---|---|
| positive | the result type javap reports | absent members, and results that are too narrow |
| negative | the next *narrower* type | results that are too wide (real scalac rejects all 727) |

**The positive probe drew exactly 10 errors and the negative probe none.** In
both `--scala-library` and `--no-scala-library`, identically. Every other
declared member, and every result type, already agreed with scalac — which is
the useful half of the answer, and is not something anyone had checked.

The ten: `Boolean.&`, `Boolean.|`, `Boolean.^`, and `unary_+` on all seven
numeric classes. `unary_+` costs the library measure nothing; the other three
cost 15 (`&` 9, `^` 6 — the library writes no `Boolean.|`).

Two things the audit says are *deliberate*, not gaps, and should not be
"fixed" by the next slice:

* **The value-class companions** (`Int.MaxValue`, `Double.NaN`, `Byte.box`,
  `int2long`, …) are absent in `--no-scala-library` and present in
  `--scala-library`. `crates/typer/src/prelude_numeric.rs` gates them on
  `library_abi` on purpose: the private runtime has no `scala/Int$` classfile,
  so declaring them there would emit calls to classes that do not exist. The
  one real gap the probe found is `Boolean.box` / `Boolean.unbox`, which fail
  in **jar** mode too; nothing in any measured corpus writes them.
* `Predef.any2ArrowAssoc` is the reverse: a member scalac does **not** declare
  (2.13's is `ArrowAssoc`), and it is the whole of the `->` defect below.

### `Boolean`'s `&`, `|`, `^` are not `&&` and `||`

`p & q` evaluates `q` unconditionally. Emitting the short-circuiting form
would type-check, verify, and pass every classfile check in this repository
while silently dropping `q`'s side effects, so
`tests/fixtures/libprelude_boolbit.scala` **runs**, with each operand
appending to a trace: `trace=LR` for `&` against `trace=L` for `&&`. Expected
output is real scalac 2.13.16's own, and both modes match it.

### `x.unary_-` written out in full never reached its intrinsic

Found while adding `unary_+`, pre-existing, and invisible to every
compile-only check. The prefix spelling `-x` is an `Apply` and has always
worked; the *selected* spelling is a bare `Select`, and `gen_select`'s
intrinsic chain had no case for the five unary value-class families. It fell
through to `invoke_method` and emitted
`invokevirtual java.lang.Byte.unary_$minus()` — a method the box does not
have. **It compiled, it verified, and it threw `NoSuchMethodError` at run
time**, on all nine members (`unary_-`/`unary_~` on Byte/Short/Char/Int/Long,
`unary_-` on Float/Double, `unary_!` on Boolean). `emit_prim_unary` is now
shared by both spellings.

### `->`: the doc's own reproduction was not a bug

The entry above prescribes item 0 with a twelve-line reproduction and two
fixes measured at zero. One command settles it:

```
$ scalac -classpath scala-library-2.13.16.jar A.scala B.scala
B.scala:2: error: type mismatch;
 found   : Int(1)
 required: ?{def ->(x$1: ? >: Int(2)): ?}
Note that implicit conversions are not applicable because they are ambiguous:
 both method ArrowAssoc in object Predef of type [A](self: A): ArrowAssoc[A]
 and method ArrowAssocQ in object PredefY of type [A](self: A): PredefY.ArrowAssocQ[A]
 are possible conversion functions from Int(1) to ?{def ->(x$1: ? >: Int(2)): ?}
```

**Real scalac rejects that program too.** Two conversions genuinely in scope
for the same source type *are* an ambiguity; "two candidates, so neither" is
the right answer there, and the two fixes that measured zero were aimed at a
case that is not wrong. It is pinned now, as a negative test
(`two_real_conversions_are_still_an_ambiguity`), so it cannot be "fixed" by a
later slice.

`src/library` is a different situation, and the difference is the whole
defect. There the program has exactly **one** `->` conversion, `Predef`'s own.
This compiler had a second — its own prelude stand-in for it. Instrumenting
`search_extension`'s tie says so directly:

```
TIE -> from="ALL" :: [("any2ArrowAssoc", 400, "Predef$", "ArrowAssoc"),
                      ("ArrowAssoc",    2869, "Predef$", "ArrowAssoc[String]")]
```

id 400 is the prelude's; id 2869 is the source `implicit final class
ArrowAssoc`. `predef_reimport` supersedes the prelude's `Predef` snapshot **by
name**, and these two names never meet, because `any2ArrowAssoc` is 2.10's
spelling — `javap -p scala.Predef$` on 2.13.16 has `public final <A> A
ArrowAssoc(A)` and no `any2ArrowAssoc` at all.

`Check::drop_superseded_prelude_conversions` drops a prelude candidate owned
by the prelude `Predef` when the run's own sources have supplied that object
(`SymbolTable::predef_superseded`) and some other candidate remains. It never
fires for an ordinary program, and it never empties a candidate set.

**Two more principled-looking shapes were measured and are not in the change**
— record them the way the entry above records its two:

| shape | measured |
|---|---|
| rename the prelude's copy to `ArrowAssoc`, so replace-by-name reaches it | **1099 / 158** |
| retire the whole prelude `Predef` snapshot once the source one arrives (nsc's actual model) | **1083 / 156** |
| drop the prelude candidate only when another remains | **1065 / 154** |

The first two are the same experiment: both remove the stand-in outright, and
both cost more than they save, because the source `ArrowAssoc` does not reach
every site the stand-in did — `t.head -> t.tail` on a bare type parameter in
`collection/package.scala` is one, and there are six of that shape. Keeping
the stand-in as a *last resort* rather than as a competitor is what gets the
whole 28 with nothing new in the log.

### `eq` / `ne` under a universal trait

The retirement experiment turned this up, and it is pre-existing:

```scala
trait Eqls extends Any { def canEqual(that: Any): Boolean }
trait Prod extends Any with Eqls
final class T2[A, B](val _1: A, val _2: B) extends Prod
def f(t: T2[Int, Int]) = t eq null      // value eq is not a member of T2[A, B]
def g(t: T2[Int, Int]): AnyRef = t      // accepted
```

`lookup_member` reaches `AnyRef`'s members only by walking a *declared*
parent, and `rough_parents` supplies `AnyRef` only when the parent list is
**empty**, so a class whose ancestry bottoms out in a universal trait has a
chain that ends at `Any`. That is `src/library`'s whole
`Tuple`/`Product`/`Iterator` family: `scala.Equals` is
`trait Equals extends scala.Any`. The compiler already agrees the receiver is
a reference — the second line is accepted — so `check_select` now asks
`AnyRef` when the receiver conforms to it, last, after the declared members
and after any view.

In the library this was masked by the stand-in above: `any2ArrowAssoc`'s
result is the prelude's `ArrowAssoc extends AnyRef`, which *does* have `eq`,
so `t eq null` was resolving through an `->` conversion. Remove one without
the other and 19 `eq`/`ne` errors appear.

`tests/fixtures/libprelude_anyref.scala` runs, because `eq` is reference
identity: `p eq q` is false where `p == q` is true, so a fallback that
resolved it to `==` would type-check, verify and print the wrong answer.

### Three defects found and not fixed here

1. **A source `scala.Predef` breaks `java.lang.System.out.println` in
   `--scala-library` mode.** Eleven lines, no `->` anywhere:

   ```scala
   package scala { object Predef { type String = java.lang.String } }
   object Main { def main(a: Array[String]): Unit = java.lang.System.out.println("hello") }
   ```

   compiles, and dies with
   `NoSuchMethodError: 'void scala.Predef$.println(java.lang.Object)'`. The
   qualified select is being given the prelude `Predef`'s receiver. Correct in
   `--no-scala-library`, and correct in jar mode without the source `Predef`,
   so it is the `predef_reimport` wildcard that codegen reads back. It costs
   the measure nothing (the measure is `--no-scala-library`) and it is why
   `libprelude_arrow.scala` is run in one mode only.

   **Fixed by `agent/sysout`; see the section below.** The guess in the last
   sentence is wrong — no wildcard and no import is involved. The receiver was
   never read at all: `gen_apply` dispatched the print intrinsic on the name
   alone.
2. **`val b: Byte = -3` is rejected** — `type mismatch; found: Int required:
   Byte`, in both modes, while `val b: Byte = 3` is fine. A negated constant
   literal is not folded before the narrowing check. Real scalac accepts it.
   Zero errors in `src/library`, so it is a correctness item, not a yield one.
3. **`def f(a: Int, b: Int) = a eq b` is accepted**, boxing through
   `int2Integer`; scalac rejects it with "the result type of an implicit
   conversion must be more specific than AnyRef". Pre-existing — an unmodified
   build of `main` at `8f1df474` accepts it — and it is in implicit search,
   not in the `AnyRef` fallback above, which asks `is_sub_type(recv, AnyRef)`
   and is never reached.

### The other targets, before and after

| target | before | after |
|---|---|---|
| scala library | `1111 / 155` | **`1065 / 154`** |
| gitbucket | `270 / 79` | `270 / 79` |
| cats | `185 / 71` | `185 / 71` |
| slick (compile) | `errors=0 classes=1490` | `errors=0 classes=1490` |

### The head of what remains, re-clustered on the 1065

18 `value + is not a member of <notype>`, 15 `found: <overload Stream[A] |
Iterable[A] | Stream[A]> required: Stream[A]`, 13 `found: T required: A`, 13
`no matching overload for (MainNode[K, V], …)Boolean`, 11 `value min is not a
member of <notype>`, 9 `incompatible type in overriding`, 8 `value max is not
a member of <notype>`, 8 `found: null required: A`, 8 `reassignment to val
initBlank`, 7 `could not optimize @tailrec`, 6 `value tail is not a member of
<overload Iterable[A] | Stream[A] | Stream[A]>`. The `->` family is gone; the
`&` / `^` family is gone.

The `<notype>` receiver (18 + 11 + 8 = 37 between `+`, `min` and `max`) is now
the largest single spelling in the log and has no entry anywhere in this
document. It is worth a slice of its own: a receiver printed as `<notype>` is
a symbol whose type was never assigned, so the site of the error is not the
root and the counts above are what one root is worth.

## The `agent/libnotype` slice: `<notype>` was three roots, and only one owed a diagnostic

**1111 errors in 155 files → 1024 in 150**, measured on this branch against the
`8f1df474` branch point. The `<notype>` cluster went **56 → 6**. gitbucket
`270 / 79`, cats `185 / 71` and slick `errors=0 classes=1490` are unmoved.

The brief handed this slice 48 errors under two headings — 18 `value + is not a
member of <notype>` and 11 `value min` — and asked "why does the receiver have
no type at all, and why was nothing reported where it was lost?". Instrumenting
the one report site (`check_select.rs`, printing the qualifier's *symbol* when
its type is `NoType`) answered it in one run of the measure, and the answer was
that the 56 errors had three unrelated causes with three different answers to
the second half of the question.

| receiver, as the symbol table had it | errors |
|---|---:|
| `Math` / `Runtime`, `kind=Package`, `jvm="scala/Math"` | 26 |
| `newCachedHashCode` / `dataToNodeMigrationTargets`, a local `var` | 26 |
| `accum`, a local `val` | 35 |

The third row is larger than its share of the `<notype>` count because most of
what it removed says something else (`no matching overload for constructor ::`).

### 1. A directory classpath entry was matched case-insensitively

`jvm="scala/Math"` is the whole diagnosis. `Typer::complete_binary_member`
treats a directory under a package as proof that a *package* of that name
exists:

```rust
if self.binary.has_package_prefix(&prefix) {
    let _ = crate::classpath::ensure_package(&mut self.st, &internal);
    return;
}
```

and `BinaryIndex::has_package_prefix` answered that with `path.join(rel).is_dir()`.
macOS's default APFS volume is case-insensitive, and
`tests/scalalib_measure.sh`'s Java classpath really does contain
`scala/math/ScalaNumber.class` and `scala/runtime/BoxesRunTime.class` — so
`<cp>/scala/Math` and `<cp>/scala/Runtime` were directories, `package
scala.Math` and `package scala.Runtime` were invented, and because the search
order is `scala` first and *then* the implicit `import java.lang._`, they
shadowed `java.lang.Math` and `java.lang.Runtime` for the whole run. Every
`Math.min` / `Math.max` / `Math.nextAfter` / `Math.multiplyExact` /
`Runtime.getRuntime` in `src/library` then selected on a package.

Nothing was reported where the name was lost, because as far as the typer was
concerned nothing had failed. The only diagnostic in the run came from the
*backend* — `cannot load Math`, for the one occurrence in a value position.

`java.lang.Math.max(1, 2)` worked throughout, and so did bare `Integer`,
`String`, `System`, `Thread`, `Character`, `StrictMath` and `Class`: this only
ever hit a `java.lang` class whose name collides case-insensitively with a
`scala.*` package on the classpath. It also only ever hit the `--no-scala-library`
arrangement, because the jar entries a zip is asked about are matched exactly.

Both directory lookups in `BinaryIndex` — the package prefix and the class file
— now verify the on-disk spelling of every component (`path_case_matches`).
It runs only after the cheap `is_dir()`/`is_file()` has said yes, and
`find_class` memoises, so the cost is bounded by the number of distinct names
actually found on a directory entry.

### 2. A blank line before a bare block was read as an argument list

nsc's scanner distinguishes `NEWLINE` from `NEWLINES` (`Scanners.pastBlankLine`),
and `newLineOptWhenFollowedBy(LBRACE)` skips only the former. So in real scalac
2.13.16

```scala
g
{ 41 }          // an application: g { 41 }

g(1)

{ 41 }          // two statements
```

Our lexer collapsed every run of line breaks into one `Newline` token, so both
were applications — and `HashMap.concat` and `HashSet.concat` are written as

```scala
var newCachedHashCode = 0

{
  …
  newCachedHashCode += newNode.cachedJavaKeySetHashCode
```

which parsed as `0 { … }`. The `var` took the failed application's type, and
all 18 `+=` and both `|=` beneath it reported on `<notype>`.

Here an error *was* printed — `value apply is not a member of 0`, at the `var` —
so this family was not silent. It was worse than silent in a different way: the
message describes a program the author did not write, at a line where nothing
is wrong.

`Token` now carries `blank_line`, set in `Lexer::emit` by the same rule nsc
uses (scan back over whitespace from the `\n`; a comment counts as content,
because nsc scans raw characters and `/` is not whitespace), and OR-ed across a
run when `drop_non_separating_newlines` collapses it. `simple_expr_rest` stops
its application loop at a blank line.

Deliberately **not** changed: `newline_opt_when_followed_by`, which the
template and `extends` parsers use. nsc rejects `class A` + blank line + `{ … }`
too (checked against real scalac), so being faithful there would turn programs
this compiler currently accepts into errors, with nothing to gain in any
measure. Worth doing on its own if the `neg` corpus ever asks for it.

### 3. `new X` was resolved in the term namespace

The unqualified-`Ident` arm of `TreeKind::New` used `SymbolTable::lookup`,
which returns the innermost scope that binds the name *at all*. `HashMap.concat`
writes

```scala
class accum extends AbstractFunction2[K, V1, Unit] with Function1[(K, V1), Unit] { … }
…
val accum = new accum
```

and in the nested block the nearest `accum` is the `val` being defined. No
`Class` among the candidates, so the arm fell through to
`self.type_expr(tpt, &Type::NoType)` — typing `accum` as an *expression*, which
found the half-built value and handed back its `NoType`. **This is the one of
the three where a diagnostic really was owed and never came**: the search
failed and the tree was left typed as if it had succeeded, which is exactly the
shape `agent/gbhead` recorded above.

`lookup_type` skips a scope that binds the name only as a term, by
construction, so the fix is to consult it when the plain lookup found nothing
in the type namespace. That also fixes the same collision in `List.scala`,
where `List` declares `def ::` and the class `::` is top-level: `new ::(x, xs)`
had been finding the method. 35 errors, of which only 2 mentioned `<notype>` —
the rest were `no matching overload for constructor ::`, `type mismatch; found:
::[Nothing]`, and `value next is not a member of ::[Nothing]`.

The narrow shape matters. `found` is replaced only when it holds no
`Class`/`TypeParam`/`TypeMember` at all, so the `only_module` path
(`object O` alone under the name, which nsc reports as `not found: type O`) and
the `class type required but T found` path are untouched, and both are pinned
in `tests/fixtures/libnotype_bad.scala`.

### Verification

`tests/fixtures/libnotype.scala` **executes and prints**, against the private
runtime and against the real library ABI, and its expected output is real
scalac 2.13.16's from compiling the same source. `libnotype_bad.scala` is
rejected at three of scalac's own four lines — 12 (the blank-line one: if the
blank line did not end the expression, `one { (x: Int) => x }` is well typed
and the file compiles clean), 16 and 19. Line 10, scalac's `missing argument
list for method one`, is a separate pre-existing laxity about eta-expansion in
this compiler and is not asserted. All four tests in
`crates/cli/tests/libnotype.rs` fail on a build of the branch point.

No error kind in the library log went **up**, and no new kind appeared — which
is unusual for a cascade fix and is worth saying, because it means none of the
87 was hiding a further error.

### The four `neg` corpus losses, audited

`tests/verify_merge.sh` reports `VERDICT=FAIL` on this branch for
`corpus losses=4`. The corpus moved `pos 1089 → 1094`, `run 618 → 621`,
`neg 674 → 670`. All four `neg` losses are the same thing: the test had been
"passing" on an error that is not in its `.check` at all, produced by one of
the three bugs above. Compiled with a build of the branch point and with this
one, side by side:

| test | what scalac reports | what we reported before | now |
|---|---|---|---|
| `anytrait` | 3 × `field/statement not allowed in universal trait` (3, 5, 9) | `recursive value x needs type`; `value apply is not a member of 1` | clean |
| `name-lookup-stable` | 2 × `reference to PrimaryKey is ambiguous` (15, 17) | `no matching overload for Nothing with arguments (PrimaryKey$)` | clean |
| `t8002-nested-scope` | `method x in class C cannot be accessed … from object C` (8) | `value x is not a member of C$` | clean |
| `valueclasses-impl-restrictions` | 3 × `implementation restriction: nested … in value class` (3, 9, 23) | `no matching overload for String with arguments ((<notype>) => <notype>)` | clean |

Three of the four are the blank-line parse (`1 { x += 1 }`, `??? { import …; … }`,
`i2.z { case x => x }`) and the fourth is `new C()` resolving to `object C`,
which is the third root exactly. The last row's old message is itself one of
the `<notype>` symptoms this slice was sent after.

What actually remains unimplemented behind them: universal-trait restrictions,
value-class nesting restrictions, name-ambiguity between a member and a
subsequent wildcard import, and `private` access checking on a *nested* class.
None was ever implemented; the corpus was crediting us for them because a
different bug happened to reject the same files. This is the `neg` upper bound
`.agent-brief.md` warns about, seen from the other side.

### What is left of `<notype>`

Six: 2 `ambiguous overload for processFully with arguments ((<notype>) =>
<notype>)` (the deliberate placeholder for a literal whose parameter types are
not yet known — `check_overload.rs` documents it), 2 `type Partial is not a
member of <notype>` (`BigDecimal.scala`'s `Range.Partial[…]`), 1 `type
BigDecimalAsIfIntegral is not a member of <notype>` (`Range.scala:608`,
`Numeric.BigDecimalAsIfIntegral`) and 1 `type mismatch; found: <overload Int |
<notype>>`. The middle two are the *type* namespace's version of the same
question and were not investigated here.

### The head of the library afterwards

| n | message |
|---:|---|
| 15 | `type mismatch; found: <overload Stream[A] \| Iterable[A] \| Stream[A]>  required: Stream[A]` |
| 13 | `type mismatch; found: T  required: A` |
| 13 | `no matching overload for (MainNode[K, V], …)Boolean` |
| 9 | `value & is not a member of Boolean` |
| 9 | `incompatible type in overriding` |
| 8 | `type mismatch; found: null  required: A` |
| 8 | `reassignment to val initBlank` |
| 8 | `no matching overload for <overload (Array[Long], Long)Unit \| …>` |
| 7 | `value -> is not a member of TimeUnit` |
| 7 | `type mismatch; found: Array[Nothing]  required: Array[Array[AnyRef]]` |
| 7 | `type mismatch; found: ((K, V)) => U  required: (K) => Any` |
| 7 | `could not optimize @tailrec annotated method` |

## The `agent/sysout` slice: a qualified `println` is a call on its own receiver

Defect 1 of the list above, and the diagnosis in that list is wrong in an
instructive way. It reads: "it is the `predef_reimport` wildcard that codegen
reads back". No wildcard, no import and no `Predef` symbol is involved. The
receiver was never consulted at all.

`gen_apply` dispatched the print intrinsic on the **name**:

```rust
if ctx.library_abi && (matches!(ic, Intrinsic::Println) || fun.name() == Some("println")) {
    gen_predef_println(asm, frame, ctx, args, true);
```

and both emitters discard the qualifier outright — `gen_predef_println` loads
`scala/Predef$.MODULE$`, `gen_println` loads `java/lang/System.out`. So every
selection spelled `println` or `print` became a call on `Predef`, whatever the
program wrote. `javap` on the eleven-line reproduction says so in four
instructions, with no trace of `System.out` anywhere:

```
0: ldc           #15    // String hello
2: getstatic     #20    // Field scala/Predef$.MODULE$:Lscala/Predef$;
5: swap
6: invokevirtual #24    // Method scala/Predef$.println:(Ljava/lang/Object;)V
```

### Why it needed the combination, and what else it was hiding

Not because the source `Predef` changed the *compilation*: it does not. The
bytecode above is byte-for-byte what an ordinary program gets too. What the
source `Predef` changes is the **run**: the compiler emits its own
`scala/Predef$.class`, which precedes the jar's on the classpath and has no
`println`, so the hijacked call finally names a method that is not there. In
every other program the jar's `Predef.println` happens to exist and happens to
print the same text, so the wrong method was invisible.

That is also why looking only at the reported symptom would have missed the
larger half. The same root sent **`java.lang.System.err.println(x)` to
stdout**, in both modes, with or without a source `Predef` — an ordinary
program silently losing the distinction between its output and its error
stream. It compiles, it passes `-Xverify:all`, and every compile-only measure
in this repository is green on it. It is the `verify_failures is a lower bound`
rule in `.agent-brief.md` again: calling the wrong method of the right shape
keeps the types consistent and the verifier has no opinion.

### The fix

`Intrinsic::Println` / `Intrinsic::Print` are set **only** on the prelude's own
`Predef` members (`prelude_predef2::add_predef_members`), and in both
`--scala-library` and `--no-scala-library` — instrumenting `gen_apply` shows
`ic=Println owner="Predef$"` for `println(x)` and `scala.Predef.println(x)`,
and `ic=None owner="PrintStream"` / `owner="Console$"` for everything else, in
both modes. The intrinsic therefore already names the exact set that may be
rewritten. The name-only fallback now applies solely to a call with no symbol
at all (`gen_expr::unresolved_print`), where there is nothing else to emit.

A source-defined `scala.Predef.println` carries no intrinsic and is now emitted
as the ordinary call it is, which is what nsc does — so a source `Predef` may
define its own `println` without recursing until the stack runs out.
`tests/fixtures/libmaxmin_predef.scala` was written around exactly that and
records it in a comment.

Two shapes that were *not* used, and why:

| shape | why not |
|---|---|
| key on the owner's internal name being `scala/Predef$` | a source `scala.Predef` erases to `scala/Predef$` too, so this claims its members back into the intrinsic and emits `(Ljava/lang/Object;)V` whatever the source declared. Measured: `def println(s: String)` in a source `Predef` compiles to `invokevirtual scala/Predef$.println:(Ljava/lang/String;)V` under the fix as shipped, and agrees with scalac; the owner rule would have named a method that is not there |
| key on the tree shape (a bare `Ident` is Predef's) | loses `scala.Predef.println(x)`, which is the intrinsic and must stay so |

### The test

`tests/fixtures/sysout_predef.scala` **runs**, in `--scala-library` mode, under
`-Xverify:all`, and both streams are compared against real scalac 2.13.16
compiling the same file against the same jar. It holds both halves in one
program on purpose: the qualified calls must reach `System.out` / `System.err`,
and the *unqualified* `println` must still resolve through the source `Predef`
that `predef_reimport` put in scope and call **that object's** method — the
`P:` / `p:` prefixes in the expected output are what tells the source `Predef`
apart from the jar's. `crates/cli/tests/sysout.rs` adds the reproduction
verbatim, the `System.err` symptom in both modes, `Console.err`, a user class
with a method called `println`, slick's `TreePrinter` shape, and an ordinary
program's `println` in both modes as the regression guard. **Six of the seven
fail on an unmodified build of `acec3f09`**; the seventh is the guard and
passes on both.

### It was miscompiling a measured corpus, silently

`slick/util/TreePrinter.scala` is the one file in slick's 184 that writes
through a `PrintWriter` it is handed:

```scala
def get(n: Dumpable) = {
  val buf = new StringWriter
  print(n, new PrintWriter(buf))     // TreePrinter's own two-argument `print`
  buf.getBuffer.toString
}
def print(n: Dumpable, out: PrintWriter = …): Unit = { …; out.println(…) }
```

Every one of those was claimed. `gen_predef_println` reads `args[0]` and drops
the rest, so `print(n, new PrintWriter(buf))` became `Predef.print(n)` — the
writer was never even constructed, `get` returned the empty string, and the
dump went to the console instead. Throughout that,
`tests/slick_measure.sh` reported `errors=0 files_with_errors=0 classes=1490`,
`tests/verify_all.sh` reported `verified=1490 failed=0`, and the classfile lint
reported `lint_problems=0`. Nothing short of running the code has an opinion
about it. It is pinned as `a_writer_passed_as_an_argument_is_written_to`.

### Yield: zero corpus tests, and that is the honest number

The full `scala/scala` corpus is **identical test-for-test** between an
unmodified `acec3f09` build and this branch — all 5324 rows, `pos` `neg` and
`run`, measured on both binaries rather than read off a ledger. Nothing in the
corpus distinguishes `Predef.println(x)` from what the program wrote, because
`Predef.println` delegates to `Console.println` which writes to `System.out`:
the text comes out right, through the wrong method, and the `.check` files
compare text.

So the gate's `losses=4 changes=15` against `tests/baselines/corpus-4d613d25.tsv`
is entirely `main`'s own drift (`agent/libprelude` and `agent/libnotype`, both
merged at `acec3f09`) and none of it is this change. The four losses are
exactly the four `agent/libnotype` declared: `neg/anytrait`,
`neg/name-lookup-stable`, `neg/t8002-nested-scope`,
`neg/valueclasses-impl-restrictions`. There is no fifth.

That a fix worth zero corpus tests is still worth making is the point of the
section: the corpus measures text on stdout, and this defect preserved the text
while replacing the method, the receiver and — in slick's case — an argument.

### The numbers

`tests/verify_merge.sh` at `38905f33`, this branch with `main` merged in three
times while it ran (`agent/neglit`, `agent/pickleparams`, `agent/negchecks`):

| check | branch point `acec3f09` | merged tree `38905f33` |
|---|---|---|
| scala library | `971 / 147` | `917 / 146` |
| gitbucket | `270 / 79` | `270 / 79` |
| cats | `185 / 71` | `185 / 71` |
| slick (compile) | `errors=0 files_with_errors=0 classes=1490` | same |
| `slick_run.sh` | `progs=12 ok=12 diff=0 fail=0 attempts=36/36` | same |
| subset + lint | — | `verified=1490 failed=0 lint_problems=0` |
| `cargo test --workspace --release` | — | `259 rows, 2487 passed, 0 failed` |
| corpus (full) | — | `pos 1095 / neg 673 / run 626` |

**None of the movement is this change's**, and there is a measurement for that
rather than an assumption: a gate run on this branch *before* any of the three
merges reported `971 / 147`, `270 / 79`, `185 / 71` and `classes=1490` with the
print fix already in — every figure the brief gave for `acec3f09` — and the
corpus was identical row for row, all 5324 of them in all three kinds, between
an unmodified `acec3f09` build and this branch.

`VERDICT=FAIL` on all four gate runs, always and only for `corpus losses`
against `tests/baselines/corpus-4d613d25.tsv`, which the ledger predates: four
in the first three runs — exactly the four `agent/libnotype` declared — and one
in the last, `neg/name-lookup-stable`, after `agent/negchecks` restored the
other three. No loss outside that set appeared in any run, and no `pos` or
`run` loss in any of them.

### Not fixed here, same root

`gen_apply` has one more name-only dispatch of the same shape, immediately
below:

```rust
if ctx.library_abi
    && (fun.name() == Some("identity")
        || fun.name() == Some("locally")
        || fun.name() == Some("implicitly")
```

`gen_predef_poly` likewise discards the qualifier and emits
`scala/Predef$.<name>:(Ljava/lang/Object;)Ljava/lang/Object;`, so a user method
called `identity` or `locally` is claimed the same way `println` was. It is not
in this change because it wants its own dual-run evidence, and mixing it in
would make the `println` numbers unreadable.
## The `agent/neglit` slice: a negated constant literal is not folded before the narrowing check

Closes the second of `agent/libprelude`'s "Three defects found and not fixed
here", above: `val b: Byte = -3` and `val b: Byte = -300` were both `type
mismatch; found: Int  required: Byte` (as were the `Short` and `Char`
equivalents), while `val b: Byte = 3` was fine. Real scalac 2.13.16 accepts
`-3`, `-4` and `65` against `Byte`/`Short`/`Char` and rejects `-300` exactly
where it rejects `300`.

**Zero errors in `src/library` before and after — the library never writes a
negated literal against a narrowing target. This is a correctness item, not a
yield one, and no measure in this document moved.** scala library `1065 /
154`, gitbucket `270 / 79`, cats `185 / 71`, slick (compile) `errors=0
classes=1490`, all unchanged from the `agent/libprelude` row above.

### The mechanism

`-3` has no negative-literal token in this parser: it desugars at parse time
to `Apply(Select(Literal(3), "unary_-"), Nil)`, the same as any other unary
operator call. `unary_-` is a real prelude method (`Intrinsic::IntUn("-")`,
`crates/typer/src/prelude_anyval2.rs`) whose *declared* return type is the
widened `Int`, so once the `Apply` is typechecked the tree's type is
`Type::Int` — a concrete type, not `Type::Constant(Lit::Int(-3))`.

`Typer::adapt` (`crates/typer/src/check_infer.rs`) is where SLS 6.26.1's
narrowing already lived: a bare literal like `3` carries `Type::Constant`
all the way to the point `adapt` applies the expected type, and a single
`if let Type::Constant(Lit::Int(v)) = &tree.ty` check there decides whether
it fits `Byte`/`Short`/`Char`. `-3` never reaches that check with a matching
`tree.ty`, because the method call in between replaced the constant with its
declared (necessarily widened) return type.

SLS 6.24 defines a constant expression to include a unary `-` applied to a
literal, so the fix recovers that fact from the tree's *shape* rather than
its type: `negated_int_literal` matches exactly `Apply(Select(Literal(Lit::
Int(v)), "unary_-"), [])` and returns `-v`. `adapt`'s existing check now asks
for that value when `tree.ty` is not already a `Constant`, and applies the
same `fits` range test either way — one narrowing rule, not two. On success
the tree becomes a plain `Literal` (the same shape a bare `3` already is),
which is why no codegen change was needed: `gen_literal` pushes the `int`
constant from the `Lit` alone, and `Byte`/`Short`/`Char` are `int` on the
JVM operand stack regardless of which literal shape produced the value.

Deliberately narrow: the match requires a literal *operand*, not merely a
constant-typed one, so `val n = 3; val b: Byte = -n` is untouched and still
reports the plain `Int`/`Byte` mismatch scalac itself gives. Widening a
negated literal to `Long`/`Float`/`Double` (`val l: Long = -3`) was already
correct before this slice — that is `numeric_widen`, not the narrowing check,
and does not care whether the `Int` it is widening is a literal or not.

### Fixtures

`tests/fixtures/negl_run.scala` is **run**, not merely compiled, in both
`--no-scala-library` and `--scala-library` mode, against `expected/
negl_run.txt` (real scalac 2.13.16's own output for the same source): a fold
landing on the right type but the wrong value — a truncation, a dropped sign
— would type-check and only show up here. It covers `Byte`, `Short`, `Char`,
the pre-existing plain-literal path alongside the new negated one, both
`Byte` boundary values (`127`, `-128`), and `Long`/`Float`/`Double` widening.

Three negative fixtures, each pinned against real scalac at the same line:
`negl_bad_range.scala` (`val b: Byte = -300`), `negl_bad_nonconst.scala`
(`val n = 3; val b: Byte = -n`), and `negl_bad_boundary.scala` (`128` and
`-129`, one step past each side of the range `negl_run.scala` accepts). All
four fixtures were confirmed to behave differently on the pre-fix binary:
`negl_run.scala` failed to compile at all before the fix (the bug), while all
three negative fixtures already failed the same way before and after (they
were never affected).

`crates/cli/tests/neglit.rs` holds all of this, kept out of `e2e.rs` per
`.agent-brief.md`. Fixtures use the `negl_` prefix.

## What to do next, in order

0. ~~**`->` when two conversions offer it — 28 errors, 3 files.**~~ Done by
   `agent/libprelude`, in the section just above, which also shows that the
   twelve-line reproduction is **not** a case this compiler gets wrong: real
   scalac 2.13.16 rejects it too. Read that section before trusting anything
   in this list that is written as a reproduction rather than a measurement.
1. **`Vector2[Any]` … `Vector6[Any]` — 100 errors, all in `Vector.scala`.**
   `new VectorN(…)` on a generic constructor infers `Any` for the element
   where the context expects `Vector[B]`. Nothing to do with the prelude; it
   is constructor type inference. `Tree[A, …]` (73, `RedBlackTree.scala`) and
   `Array[Any]` (43) look like the same shape and should be checked together.
2. `case class` synthesis does not produce `canEqual`, so all 22 `TupleN`
   classes report `class TupleN needs to be abstract` against
   `Product`/`Equals`. 22 errors, one root, and it needs no lookup work.
3. The overriding family was *partly* a second root after all. The
   `agent/libanyval` slice above removed 34 of it — every
   `` `override` modifier required`` and every `cannot override final
   member`` — by fixing the matcher rather than by lookup work. What is left
   of it is 9 `incompatible type in overriding` plus 10 `overrides nothing`,
   and those *are* the member-lookup bug seen from the other side.
4. ~~**`SymbolTable::base_type_args` takes the first path, not the meet.**~~
   Done by `agent/basetypemeet`, in the section below: 917 -> 912, and the
   base type of a class reached twice is now nsc's contravariant merge. The
   half of this item that named the `<overload …>` receivers was **wrong**,
   and that slice measured it: `<overload Nil$ | Nil$>` (7), `<overload Set[A]
   | TreeSet[A]>` (3) and `<overload Iterable[(K, V)] | Map[K, V] |
   TreeMap[K, V]>` (3) are two *symbols*, not two instantiations — two sibling
   overrides of one inherited member, which `drop_overridden` has no rule for.
   That is a member-collapse question after all, and it is the next one.
5. `src/reflect` and `src/compiler` are not worth measuring yet.
   `SCALALIB_DIRS` accepts them when they are.

## The `agent/liboverload` slice: re-abstracting is overriding

**970 errors in 147 files → 917 in 146**, measured on this branch merged with
`main` at `659580a9` (`agent/libprelude`, `agent/libnotype`, `agent/neglit`)
against that same `main` measured on its own. The delta is the same **-53** it
was at the branch point (`fb297e74`, 1104 → 1051) and at `acec3f09`
(971 → 918), so the waves do not overlap. Every other
target is unchanged to the error, and slick's 1490 class files are
byte-identical.

The brief handed this slice two clusters — 21 `type mismatch; found:
<overload Stream[A] | Iterable[A] | Stream[A]>` and 23 `value <member> is not
a member of <overload Iterable[A] | Stream[A] | Stream[A]>` — and asked which
of the known prelude collisions it is: a forwarder (`agent/catstail`), a
supply seam, a prelude/source duplicate (`agent/preludeshadow`,
`agent/libmaxmin`), or something else.

### It is none of them

One trace of the candidate set settles it. Selecting `tail` inside `Stream`
offers three symbols:

```
sym=5798  owner=IterableOps    ty=C          deferred=false
sym=6347  owner=LinearSeqOps   ty=C          deferred=true
sym=11876 owner=Stream         ty=Stream[A]  deferred=true
```

`prelude_end` is **1195**. All three owners are source classes of the library
under compilation, none has a `pickled_origin`, none has a JVM descriptor, and
there is no jar in this mode. They are the library's own override chain:
`IterableOps` defines `def tail: C = ...` (`Iterable.scala` 527),
`LinearSeqOps` re-declares it abstract (`LinearSeq.scala` 54), and `Stream`
re-declares it again (`Stream.scala` 34). **Re-abstracting is overriding, and
nothing had told `drop_overridden` so.**

### The two rules cancelled out

`Check::drop_overridden` filters each candidate against every other. Two of
its rules answered this pair in opposite directions:

* the declaration/definition rule dropped `LinearSeqOps.tail` and
  `Stream.tail` because they are declarations standing next to a definition —
  the rule `agent/gbshape` added for gitbucket's self-typed
  `Profile.profile` / `ProfileProvider.profile`;
* the owner rule dropped `IterableOps.tail` because `LinearSeqOps` is below
  it, and `LinearSeqOps.tail` because `Stream` is below *it*.

Every candidate was dropped, `kept` came out empty, and the `kept.is_empty()`
fallback — which exists so that a mutually-eliminating set does not leave the
caller indexing into nothing — handed back the **whole unreduced set**. That
is why the printed receiver has three alternatives and repeats a type: it is
not an overload at all, it is the candidate list nobody reduced.

Both rules now go through `definition_outranks_declaration`, so exactly one of
them fires.

### Which one wins is nsc's answer, not the hierarchy's

The obvious fix — "the hierarchy decides; take the most derived declaration" —
is wrong, and scalac 2.13.16 says so in four lines:

```scala
trait A { def f: A = null }
abstract class C extends A { override def f: C; def tag: Int = 1; def g = f.tag }
// error: value tag is not a member of A
```

nsc's `findMember` stops replacing a member it has already found once either
side is `DEFERRED`, so a declaration that *narrows* a concrete inherited
member does not become the member: `f` there is an `A`. The same is true when
the definition is generic and the declaration narrows past the instantiation
(`trait Holder[+C] { def get: C = ??? }`, `class NarrowBox extends
Holder[Any] { override def get: NarrowBox }` — nsc reports `Any`). Taking the
most derived declaration removed the same **53** library errors and **5** cats
errors, and diverged from scalac on both shapes; it is not in the change.

What is in the change is narrower: **a declaration that only *restates* the
definition is one member.** `Stream.tail: Stream[A]` is exactly what
`IterableOps.tail: C` says at that prefix, so which symbol survives cannot be
observed — except that reaching the answer through the definition needs the
prefix, and that is the part this compiler gets wrong (below). So the
declaration is preferred exactly where it cannot change the answer, because it
already carries it written out. cats is then unchanged at 185, which is
correct: its five were the shape nsc rejects.

### A second defect, found and not fixed: as-seen-from takes the first path

`SymbolTable::base_type_args` walks the linearization and keeps the **first**
instantiation of each base class it meets (`or_insert_with`). `Stream` reaches
`IterableOps` as both `IterableOps[A, Stream, Stream[A]]` and `IterableOps[A,
Iterable, Iterable[A]]`, and takes the second — which is why
`IterableOps.tail` prints as `Iterable[A]` in the unreduced overload above,
and why `Check::base_type_instance`, which stops at the first parent that
reaches the target, cannot be used to decide "restates" either. nsc's
`baseType` takes the *meet*, which for a covariant parameter is the most
derived. `Check::restated_on_some_path` asks whether **some** path spells the
declaration's own type, which gets the same answer here without changing what
`baseType` means everywhere else; fixing `base_type_args` is the real repair
and is a slice of its own. Building the rule on the first path instead leaves
16 of the 53 (`value tailDefined is not a member of Iterable[A]`,
`found: Iterable[A] required: Stream[A]`) — measured.

The ordering matters and is what makes the defect reproducible in nine lines:

```scala
trait IterOps[+A, +C] { def tail: C = ??? }
trait Iter[+A] extends IterOps[A, Iter[A]]
trait LinOps[+A, +C] extends IterOps[A, C] { def tail: C }
trait Str[+A] extends LinOps[A, Str[A]] with Iter[A] {
  def tail: Str[A]
  def flag: Boolean
  def go: Boolean = tail.flag   // value flag is not a member of
}                               // <overload Iter[A] | Str[A] | Str[A]>
```

Put `Iter[A]` first instead and the linearization reaches `IterOps` through
`LinOps`, the three as-seen-from types collapse to `Str[A]`, and the unreduced
set is harmless. That is why the same shape written the other way round
compiles on the pre-fix binary.

### Fixtures

`tests/fixtures/libov_reabstract.scala` is the shape above, made to run: each
of `second`, `third` and `lastOne` walks `tail`, so an alternative that merely
type-checks cannot pass, and it carries the gitbucket self-type pair as well,
which the rule must still collapse the other way. It runs in **both** modes
and against real scalac 2.13.16, byte for byte on stdout. On the pre-fix
binary it does not compile: four errors, three of them
`<overload Iter[A] | Str[A] | Str[A]>`.

`tests/fixtures/libov_reabstract_bad.scala` is the restriction. `bad1` and
`bad2` are the two nsc shapes above — this compiler now reports scalac's own
`Ops` and `Any`, where the pre-fix binary named the unreduced candidate set.
`bad3` keeps an inherited alternative the subclass does not override, and
`bad4` keeps a genuine ambiguity ambiguous; both are unchanged by the rule and
are there to say so. All four are rejected at scalac's own lines 25, 35, 45
and 54, which `crates/cli/tests/liboverload.rs` asserts against scalac
directly.

### What moved

The 50 `<overload …Stream…>` and `<overload …LinearSeq…>` receivers are gone,
together with the five `could not optimize @tailrec` that `agent/libtailrec`
traced to them. **`Stream.scala` goes from 51 error lines to 1**, and it is
the only file that changes.

Re-clustered on the 918 that remain: 435 `type mismatch`, 179 `no matching
overload`, 145 `X is not a member of Y`, 33 `not found`, 18 `ambiguous
overload`. The largest `type mismatch` families are 43 `found:
Array[Nothing]` (`new Array(WIDTH)` not reading its element type from the
expected type, mostly `Vector.scala`) and 41 `found: T`
(`Iterator.empty.next()` leaving the element parameter uninstantiated). The
worst files are `TrieMap.scala` (47), `Vector.scala` (45), `Seq.scala` (31)
and `Factory.scala` (31).

The `<overload …>` spellings left are all the *other* defect this slice
found — two instantiations of one base, not two symbols: `<overload Nil$ |
Nil$>` (7), `<overload Set[A] | TreeSet[A]>` (3), `<overload Iterable[(K, V)]
| Map[K, V] | TreeMap[K, V]>` (3). No member-collapse rule can reach those;
`base_type_args` is where they live.

### The other targets, before and after

Measured on the merged tree at `659580a9`, each against that same `main`:

| | before | after |
|---|---|---|
| scala library (538) | 970 / 147 | **917 / 146** |
| gitbucket (353) | 270 / 79 | 270 / 79 |
| cats (339) | 185 / 71 | 185 / 71 |
| slick (184) | `errors=0 classes=1490` | `errors=0 classes=1490`, all 1490 byte-identical (`SLICK_OUT` on both binaries, `diff -r` empty) |

On the scala/scala corpus (`CORPUS_SIZE=full`, 5324 rows) **every row is
identical** to `main` -- `pos 1095 / neg 670 / run 623`, compared row by row
for all three kinds at `acec3f09`, not only by count, and unchanged again at
`659580a9`. `tests/verify_merge.sh` still reports `VERDICT=FAIL` there,
because its ledger is `tests/baselines/corpus-4d613d25.tsv` and `main` has
moved three times since: the
4 `neg` losses (`anytrait`, `name-lookup-stable`, `t8002-nested-scope`,
`valueclasses-impl-restrictions`, all `accepted-but-should-not-compile`) and
the 11 `pos`/`run` gains it names all reproduce on unmodified `main`. The
ledger needs re-taking; none of the 15 is this slice's.

## The `agent/intrinsicqual` slice: the second name-only dispatch, closed

This closes the "Not fixed here, same root" item at the end of the
`agent/sysout` section above. `gen_apply` had one more dispatch of the same
shape immediately below the print one:

```rust
if ctx.library_abi
    && (fun.name() == Some("identity")
        || fun.name() == Some("locally")
        || fun.name() == Some("implicitly")
        || matches!(ic, Intrinsic::Identity | Intrinsic::Locally | Intrinsic::Implicitly)
            && fun.name().is_some_and(…))
```

The second disjunct is subsumed by the first, so the symbol was never
consulted at all, and `gen_predef_poly` emits
`scala/Predef$.<name>:(Ljava/lang/Object;)Ljava/lang/Object;` — the identity
function — **discarding the qualifier and every argument but the first**.

### What that was worth, measured by running it

`tests/fixtures/intrinsicqual_predef.scala` holds a user-defined `identity` /
`locally` / `implicitly` on an object and on an instance, beside `Predef`'s
own, in one program. Real scalac 2.13.16 and this branch both print:

```
O-identity:a … C-identity2:g|h  C-identity2:i|j  <mk:m><arg:i><arg:j>  k l m n o ev 7 8
```

An unmodified build of the branch point, in `--scala-library` mode, prints

```
a  b  c  d  e  f  g  i  <arg:i>  k l m n o ev 7 8
```

— every user method replaced by the identity function, the receiver `mk("m")`
never evaluated, and the second argument `side("j")` never constructed. In
`--no-scala-library` mode the branch point is already correct, because the
whole dispatch sits inside `if ctx.library_abi`; the fixture is run and
compared in **both** modes anyway, and both now match scalac byte for byte.

The sharpest single case is the exact analogue of `agent/sysout`'s
reproduction, one intrinsic over: a program that defines its own
`scala.Predef` with a *monomorphic* `def identity(a: String): String` emits a
`scala/Predef$.class` that precedes the jar's, and the hijacked call names a
descriptor that is not there. Before the fix it compiled, verified, and threw
`NoSuchMethodError: 'java.lang.Object scala.Predef$.identity(java.lang.Object)'`;
now it prints `P:x`.

### The fix, and why it is the same shape as `unresolved_print`

`gen_expr::predef_poly_name` returns the intrinsic's name only when the call
either **carries** `Intrinsic::Identity` / `Locally` / `Implicitly` — which
`prelude_predef2::add_predef_members` sets on the prelude's own `Predef`
members and nowhere else — or has **no symbol at all**, where there is nothing
else to emit. The name test stays beside the intrinsic test because
`Intrinsic::Identity` is shared with unrelated members (`AnyVal.toString`,
`unary_+`, `Using.resource`) that `gen_predef_poly` must never claim.

### Yield: zero corpus tests, one byte of slick, and neither is the point

**slick's 1490 class files are byte-identical** between the branch point and
this change alone (`SLICK_OUT=… tests/slick_measure.sh` on both binaries,
`diff -r`, exit 0): nothing in slick's 184 files names `identity`, `locally`
or `implicitly` on a receiver of its own. `errors=0 files_with_errors=0
classes=1490` before and after. The four class files this branch does move are
the *constructor* change below, and they are one byte each.

### Found here, not fixed here

`println(identity(()))` is a `VerifyError: Operand stack underflow`, on an
unmodified build of the branch point as well as on this one.
`gen_predef_poly` pops its own result when the result type is `Unit`, which is
right in statement position and wrong when the value is an argument. It is one
line from the fix and is left out because it is not this slice's defect and
would make the numbers above unreadable; `crates/cli/tests/intrinsicqual.rs`
names it in the guard test that would otherwise carry it.

`new Sec("abcd")`, where `class Sec(val a: Int)` also declares `def this(s:
String)`, is a `VerifyError: Bad type on operand stack`. Also pre-existing,
also unrelated to access: it reproduces with no modifier on the secondary
constructor at all. Closed by `agent/secondaryctor` below -- where the
diagnosis offered here ("emits an `invokespecial` at the *primary*
constructor's descriptor") turned out to be wrong: the descriptor was right
all along and the *argument* was adapted against the primary.

## The `agent/basetypemeet` slice: a base class reached twice is the meet

**917 errors in 146 files -> 912 in 145**, measured on this branch merged with
`main` at `5dc7f703` (`agent/nameamb`, `agent/intrinsicqual`) against that same
`main` measured on its own, and the same **-5** it was at the branch point
(`e76b0ebf`), so the waves do not overlap. cats **185 -> 182**, gitbucket
**270 -> 270**, slick `errors=0 files_with_errors=0 classes=1490` with all
1490 class files byte-identical (`SLICK_OUT` on both binaries, `diff -r`
empty). The scala/scala corpus is unchanged: `pos 1095 / neg 673 / run 626`,
`CORPUS_SIZE=full`, `losses=0 changes=0` against
`tests/baselines/corpus-3fd80269.tsv`.

The brief was item 4 of the list above, and `agent/liboverload`'s claim that
**every** `<overload ...>` receiver left in the log is this defect. That claim
is wrong, and the measured yield says so: 5 lines, not the 13 the `<overload
Nil$ | Nil$>` / `<overload Set[A] | TreeSet[A]>` / `<overload Iterable[(K, V)]
| Map[K, V] | TreeMap[K, V]>` families come to. Those families are a **third**
root; the last section here says what it is.

### The rule, and why "most derived reacher" is not it

`agent/basetypeseq` made `SymbolTable::base_type_args` walk the linearization
and keep the **first** instantiation offered for each base class, on the
grounds that `linearize` lists a class before all of its ancestors, so the
first arrival is the one the most derived *reacher* supplies. That fixes
`HashMap`, where `AbstractMap` and `StrictOptimizedMapOps` do stand in a
hierarchy. It cannot fix a base two **siblings** reach:

```scala
trait IterOps[+A, +C] { def me: C; def tail: C = me }
trait Iter[+A] extends IterOps[A, Iter[A]]
trait LinOps[+A, +C] extends IterOps[A, C]
final class Str[+A](val a: A) extends LinOps[A, Str[A]] with Iter[A] {
  def me: Str[A] = this
  def onlyOnStr: String = "Str"
  def check: String = tail.onlyOnStr   // value onlyOnStr is not a member of Iter[A]
}
```

`L(Str)` is `Str, Iter, LinOps, IterOps` -- `Iter` is first because it is
written **last** -- and neither `Iter` nor `LinOps` is above the other, so the
order says nothing about which instantiation of `IterOps` is right. scalac
2.13.16 accepts that file. In the library it is `Stream`, which reaches
`IterableOps` at `Stream[A]` through `LinearSeqOps` and at `Iterable[A]`
through `Iterable`.

nsc's answer is not an order at all. `BaseTypeSeqs.compoundBaseTypeSeq` keeps
**every** variant a base class is reached at (`minTypes`) and stores the entry
as a lazy `RefinedType`; `BaseTypeSeq.apply` then resolves it with

```scala
mergePrefixAndArgs(variants, Variance.Contravariant, lubDepth(variants))
```

which, position by position and by the *parameter's* variance against that
contravariant direction, takes `glb` at a covariant parameter (the most
derived arrival), `lub` at a contravariant one (the least derived), and for an
invariant parameter builds an existential over `TypeBounds(glb, lub)`.
`SymbolTable::meet_base_args` is that rule. The invariant case keeps the first
arrival instead: two different arguments there is an illegal inheritance, so a
program without that error is unaffected and one with it is the inheritance
check's business.

**Both halves are load-bearing.** A rule that simply took the most derived
arrival gets the covariant half right and the contravariant half backwards:
`trait Both extends Wide[Animal] with Narrow` (where `Wide[-T] extends Sink[T]`
and `Narrow extends Sink[Dog]`) reaches `Sink` at `Dog` and at `Animal`, and
`take` accepts an `Animal` -- the **lub**. `tests/fixtures/btmeet_basetypemeet.scala`
runs both.

Nothing here reorders the linearization. `agent/basetypeseq` measured that
(1551 -> **1579**, "the order that is right at one node is not right at the
node below") and threw it away; the merge does not care what order the
variants arrive in.

### `@uncheckedVariance` is a spelling, not a variant

`IterableFactoryDefaults[+A, +CC[x]] extends IterableOps[A, CC, CC[A
@uncheckedVariance]]` reaches `IterableOps` before `SeqOps` does in `Stream`'s
linearization, so the first arrival is `Stream[A @uncheckedVariance]` and the
second is `Stream[A]`. nsc's `=:=` does not look at the annotation, and scalac
prints the base type without it -- ask scalac for `override` on a method that
narrows `IterOps.tail` and it says `def tail: Restated[A] (defined in trait
IterOps)`. So the merge folds arrivals that differ only by annotations into one
and keeps the plain spelling.

That is not cosmetic. `Check::declaration_restates_definition` compares the
definition read at the declaration's prefix with the declaration's own type
**structurally**, and `Restated[A @uncheckedVariance]` is not `Restated[A]`.
With the annotated spelling winning the merge, `agent/liboverload`'s whole -53
comes back (917 -> **964**, measured).

### The 512-node walk is gone

`Check::base_type_instance` had the same first-path defect, and it is why
`Check::restated_on_some_path` existed: with no way to ask for *the* base type,
`declaration_restates_definition` asked whether **some** path from the
declaration's owner to the definition's spells the declaration's type, over a
walk whose path count is exponential in the depth of the parent DAG, budgeted
at 512 nodes, run inside overload resolution. `base_type_instance` now reads
the merged entry -- but only when more than one parent clause can reach the
target and the target has type parameters at all; a plain chain keeps the cheap
walk, because `base_type_args` linearizes and this runs in pattern typing.
`restated_on_some_path` is deleted, and the library measure is identical with
it and without it.

### What moved, and what it cost

Five library lines: `Cannot prove that (K, Any) <:< (K, String)`, two `value
unwrap is not a member of Iterable[Char]`, `found: CC[A] required: CC[B]` and
`found: CC[B (defined in class Map)] required: CC[B (defined in method map)]`.
No line appeared anywhere. cats loses three and **rewrites six more**, every
one of them from a base to something below it -- `found: Iterable[B]` becomes
`found: Seq[B]`, `Iterable[Tuple2[A, B]]` becomes `Set[Tuple2[A, B]]`,
`Iterator[Iterable[A]]` becomes `Iterator[Seq[A]]` -- which is the direction
scalac agrees with and the `sliding`/`grouped` family `docs/cats.md` separated
out.

It costs **about 10% of compile time** on `src/library`: 1.66s -> 1.82s of user
CPU on a quiet machine, min of five alternating runs, and 7-16% on a machine
with four other slices measuring on it (the numbers here were taken both ways,
because the spread between the two is larger than the effect).
`base_type_args` runs on every `subst_as_seen_from`, so the merge is on the
hot path. That it is on the hot path at all was established rather than
assumed: an env-gated build with the old first-arrival body restored ran at
`main`'s speed with everything else in place, and `sample` put
`meet_type -> is_ancestor_of` on the profile. Four things were measured and kept: one map with one hash lookup per
parent clause instead of two (a second map cost 20% on its own), the arguments
lifted out of the slot rather than cloned, an allocation-free fast path for the
argument positions every arrival agrees on (nearly all of them), and
`class_reaches` in place of `is_ancestor_of` for the ancestry test plus a memo
of its answers for the walk -- `is_ancestor_of` allocates a hash set per call
and the same pair decides several positions of several base classes. Before
those the same change cost 25%.

### The third root: two sibling *overrides* of one member

The `<overload ...>` receivers `agent/liboverload` left are **not** this
defect. `TreeSet.scala` 85/90/96 report `type mismatch; found: <overload
Set[A @uncheckedVariance] | TreeSet[A @uncheckedVariance]> required:
TreeSet[A]` for a bare `empty`, and the two alternatives are two different
symbols: `IterableFactoryDefaults.empty: CC[A @uncheckedVariance]` reached at
`CC = Set`, and `SortedSetFactoryDefaults.empty: CC[A @uncheckedVariance]`
reached at `CC = TreeSet`. Neither trait derives from the other -- they are
siblings, each overriding `IterableOps.empty` -- so `drop_overridden`'s owner
test cannot order them and its declaration/definition test does not apply
(both are definitions). nsc's `findMember` walks the base type sequence and
keeps the first match, which is `SortedSetFactoryDefaults`'s because it is the
more derived of the two *in `TreeSet`'s linearization*. That is a fourth
member-collapse rule, not a base-type question, and the base types here are
already right.

## The `agent/siblingover` slice: the receiver's linearization, and only it

**912 errors in 145 files -> 894 in 145**, measured against `main` at
`fd65f6f7` and again on the tree merged with `main` at `2182094e`
(`agent/unitpop`, `agent/secondaryctor`), where that `main` is still 912/145
and the merge is still 894/145 -- the same **-18**, so the waves do not
overlap. cats **182 -> 182**, gitbucket **270 -> 270**, slick `errors=0
files_with_errors=0 classes=1490` with all 1490 class files byte-identical
(`SLICK_OUT` on both binaries, `diff -r` empty, on both trees). Eighteen lines
removed and **none added anywhere**.

The scala/scala corpus is unchanged: `pos 1095 / neg 681 / run 627`,
`CORPUS_SIZE=full`, and `compare_corpus.py` against that same `main`'s own
full run reports `changes: [] losses: 0`. The merge gate reports nine
`fail -> pass` changes because its ledger is `corpus-3fd80269.tsv`, three
merges back; every one of the nine is already there at `2182094e`.

The brief was the section above. It was right about the root and about which
sites it explains; it was wrong about one family, and the split is below.

### The rule

`Check::drop_sibling_overrides`, run at the end of `drop_overridden` and given
the class the lookup was about: among the candidates the older rules could not
order, drop the one whose owner comes **later** in the receiver's
linearization. `crate::lin::linearize` already computes that order.

That the answer belongs to the receiver rather than to the pair is measured,
not argued. One pair of traits, two classes differing only in mixin order:

```scala
trait Ops { def emp: Ops }
trait Fac1 extends Ops { override def emp: Ops = { println("Fac1.emp"); this } }
trait Fac2 extends Ops { override def emp: Ops = { println("Fac2.emp"); this } }
class Later   extends Fac1 with Fac2   // scalac 2.13.16 runs Fac2.emp
class Earlier extends Fac2 with Fac1   // scalac 2.13.16 runs Fac1.emp
```

so no property of `Fac1` and `Fac2` alone can produce scalac's answer. Both
lines are in `tests/fixtures/sibover_siblingoverride.scala`, which is run and
compared against real scalac by `crates/cli/tests/sibover.rs`.

Writing the two parents so that the *wider* override heads the linearization
is rejected by scalac ("incompatible type in overriding"), so in any program
that compiles, "first in the linearization" and "narrowest type" coincide. The
linearization is the formulation kept because it is nsc's and because it is
still an answer when the two types are equal, as in `Later` above. (This
compiler accepts that rejected class, before and after this slice: a gap in
the override check, not in member lookup.)

### The three guards, one per slice that got this wrong

  * **Both candidates must be definitions.** `agent/liboverload` measured "the
    hierarchy decides, take the most derived declaration" and found it diverges
    from scalac, which stops replacing once either side is `DEFERRED`. A
    declaration beside a definition stays `definition_outranks_declaration`'s.

  * **The owners must be unrelated**, asked with the *same* `is_sub_type`
    predicate the owner rule uses, so the two rules cannot both fire on one
    pair or both decline it. `class_reaches` is the cheaper walk and was tried
    first: it answers `None` for the whole `*FactoryDefaults` family, whose
    parent lists it cannot follow, and the rule never fired on the twelve sites
    it was written for.

  * **They must both override a member that is in the candidate set**, *and*
    have the same parameter list once substituted at the receiver
    (`Check::same_member_at`). `same_signature` deliberately lets a parameter
    mentioning a type parameter match anything, having no prefix to substitute
    at; this rule has a receiver, so it can ask the real question.
    `agent/libanyval`'s lesson -- "these two are the same member" needs
    evidence, not a shape test -- has teeth here, and the version without
    `same_member_at` was **measured deleting a genuine overload**:

    ```scala
    trait GBase[T] { def g(x: T): String = "GBase" }
    trait GA[T] extends GBase[T] { override def g(x: T): String = "GA" }
    trait GB extends GBase[String] { def g(x: Int): String = "GB" }
    class GBoth extends GA[String] with GB
    ```

    `T` matches `Int`, `GBase.g` stands above both, every other guard admits,
    and `b.g("s")` became `no matching overload for (Int)String with arguments
    ("s")` -- the shape that took `agent/catstail`'s slick from 0 errors to 7.
    The library measure does not contain this program and did not catch it; a
    fixture did.

### The honest split of the families the brief listed

Twenty `<overload ...>` error lines were in scope. **Twelve are this root and
eight are not**, and the eight are not the mechanism `agent/liboverload`
suggested either.

| n | family | root |
|---:|---|---|
| 3 | `<overload Set[A] \| TreeSet[A]>` | sibling overrides |
| 3 | `<overload Iterable[(K, V)] \| Map[K, V] \| TreeMap[K, V]>` | sibling overrides |
| 2 | `<overload SortedMap[K, V] \| Map[K, V] \| Iterable[(K, V)]>` | sibling overrides |
| 2 | `<overload Map[K, V] \| Iterable[(K, V)]>` | sibling overrides |
| 2 | `<overload Iterable[(K, V)] \| VectorMap[K, V]>` | sibling overrides |
| 7 | `<overload Nil$ \| Nil$>` | **package member vs package object** |
| 1 | `<overload List$ \| List$>` | **package member vs package object** |

The twelve are every bare or selected `empty` in `TreeMap`, `TreeSet`,
`VectorMap` and the four `WithDefault` classes, and their candidate sets are
always `IterableOps.empty` plus two or three of
`IterableFactoryDefaults` / `MapFactoryDefaults` / `SortedMapFactoryDefaults` /
`SortedSetFactoryDefaults` -- the base correctly dropped by the owner rule,
the siblings left standing.

The `Nil` family is something else entirely. Its two candidates are `Nil` the
`Module` owned by the package `scala`, and `Nil` the `Term` owned by
`package$` -- the package object's `val Nil = scala.collection.immutable.Nil`.
One entity supplied twice, by two different routes; the owners are a package
and a package-object class, which stand in no `extends` relation, so no
ordering rule can apply and none should. The repeated type in the display is
what a duplicate *supply* looks like. It is also **not** the mechanism
`agent/liboverload` described (two rules cancelling and the `kept.is_empty()`
fallback returning everything): instrumenting `drop_overridden` over the whole
library run recorded **zero** empty-`kept` events. The set is not
mis-*reduced*; it is never reducible, because it should never have had two
members. That is a supply-seam question and it is untouched here.

### What else moved

Six of the eighteen are not `empty`: three `<overload String | ()String>`
(`toString` on a `StringBuilder` and two others, a parameterless `val` beside a
nullary `def` -- `same_signature` matches those on purpose) and three
`ambiguous overload for fromSpecific`. All six are the same root, and all six
are sites real scalac compiles.

### What it costs

Nothing measurable. Compiling `src/library`, min of eight, alternating both
which binary runs and the order inside each round: **1.85s before, 1.85s
after**, spread 1.85-1.89 on either side. The same battery on a machine with
four other slices measuring on it read 2.87s against 2.90s (+1%), with a
1.85-3.09 spread across the session -- which is why the order is alternated
and why both numbers are here rather than only the flattering one.

`drop_overridden` runs on every member selection, and on `src/library` some
twenty thousand of them arrive with more than one candidate still standing
while twelve are this defect. So the receiver's prefix is not built and the
linearization is not walked until an allocation-free scan
(`Check::could_be_sibling_pair`: owners differ, both concrete, same arity)
finds a pair that could possibly be this rule's business.
## The `agent/unitpop` slice: a `Unit` intrinsic and its discard disagreeing

The first of the two left above, closed. `gen_call::gen_predef_poly` ended

```rust
if is_unit_like(result_ty) { asm.pop(); }
else { maybe_unbox_erased_result(asm, ctx, PREDEF_POLY_DESC, Some(result_ty)); }
```

`Unit` is the one result where the erased `(Object)Object` descriptor still
returns a reference — `Predef.identity(())` hands back `BoxedUnit.UNIT` — so
the `pop` always fired at `identity(())`. Dropping it is right for a
**statement** and wrong for an **argument**: `gen_predef_println` had already
been told by `unit_leaves_boxed_ref` that its argument left a reference, so it
emitted no `BoxedUnit` of its own, and `println(identity(()))` handed
`scala/Predef$.println` an empty stack (`VerifyError: Operand stack
underflow`). nsc leaves the value — `javap` on real scalac 2.13.16 reads
`invokevirtual identity; invokevirtual println`, with no cast between them —
and lets the generic statement-position discard take it. So does this now:
`gen_predef_poly` always leaves what its descriptor promises, and
`gen_expr::discarded_predef_poly` pops it in `gen_stat`, beside
`unit_stat_leaves_ref` and `unit_call_leaves_ref`. It resolves the call head
the way `gen_apply` resolves it (`flatten_apply_owned` over `peel_fun`, then
`predef_poly_name` on the callee's `Intrinsic`), so the emitter that pushes
and the discard that pops cannot disagree about which calls are covered.

**The private runtime was failing the mirror image of the same disagreement**,
and the same predicate closes it. `--no-scala-library` inlines the intrinsic
(`gen_apply`'s `Intrinsic::Identity` arm is a bare `gen_expr` of the
argument), erasure boxes a `Unit` argument, and `unit_stat_leaves_ref` refuses
every symbol that carries an `Intrinsic` — so a discarded `identity(())` was
`getstatic BoxedUnit.UNIT` with nothing after it. Straight-line code merely
leaked an operand slot, which is why it survived; the first control-flow join
after it did not:

```scala
def f(b: Boolean): Unit = if (b) identity(()) else side("f")
// VerifyError: Inconsistent stackmap frames at branch target 18
```

A user-defined `def myid[A](a: A): A = a` in the same position was already
right (`getstatic UNIT; invokevirtual myid; pop`) — only the intrinsic was
exempt. `discarded_predef_poly` therefore mirrors the emitters arm for arm:
under `library_abi` the `(Object)Object` invoke always leaves a value, while
the private runtime leaves whatever the argument left
(`unit_leaves_boxed_ref`), except a `locally { … }` thunk, whose `Unit` result
that arm already pops where it emits it.

**Only running the program catches any of this.** Both shapes compile clean in
both modes; the classfile is well-formed and `-Xverify:all` or execution is
the first thing that objects. `tests/fixtures/unitpop_intrinsic.scala` holds
both positions in one program — `identity(())` as an argument, and
`identity(())` / `locally { … }` discarded as a statement, each followed by a
branch — plus the same three intrinsics at a non-`Unit` type so the two paths
cannot drift, and a discarded intrinsic in an `if` branch, a `match` arm, a
`try` body, a `while` body and a whole method body. It runs under
`-Xverify:all` in both modes and matches real scalac 2.13.16's output line for
line. An unmodified build of the branch point compiles it in both modes and
fails to run it in both: `Operand stack underflow` at `Main$.a3` with the jar,
`Inconsistent stackmap frames` at `Main$.s7` without it. The case commented
out in `crates/cli/tests/intrinsicqual.rs` is enabled.

**slick's 1490 class files are byte-identical** between a pre-fix and a
post-fix binary (`SLICK_OUT=… tests/slick_measure.sh` on both, `diff -r`, exit
0): nothing in slick's 184 files calls `identity` / `locally` / `implicitly`
at `Unit`. `errors=0 files_with_errors=0 classes=1490` on both.

### Found here, not fixed here

`new Sec("abcd")` — the second defect left above — is **not** the same
mechanism and is left reduced. On this build the `invokespecial` descriptor is
in fact correct; what is wrong is the *argument*, which arrives already
adapted to the **primary** constructor's parameter type:

```text
ldc "abcd"; checkcast java/lang/Integer; invokevirtual Integer.intValue;
invokestatic Integer.valueOf; invokespecial Sec."<init>":(Ljava/lang/String;)V
```

That `$unbox`/`$box` pair is in the tree before the backend sees it —
`gen_new` takes its parameter types from `ctor_param_tys`, which reads the
selected `<init>`'s own `Type::Method` and is right here — so the defect is in
the typer's overload resolution for `new`, which types the arguments against
the primary constructor while selecting the secondary one for the call.
Reproduces in both modes, byte-identical before and after this slice.

**Closed by `agent/secondaryctor` below**, which reached this same reading
independently and found the exact line: `erasure::method_param_types` takes
the class's *first* `<init>` member for a `new`, which is always the primary.
The two diagnoses agree in full — the descriptor was right, the argument was
adapted against the wrong constructor.

## The `agent/secondaryctor` slice: `new C(...)` on a secondary constructor

This closes the second "Found here, not fixed here" item above. The
reproduction is `agent/intrinsicqual`'s, unchanged:

```scala
object Main {
  def main(a: Array[String]): Unit = println(new Sec(1).viaSecondary.a)
}
class Sec(val a: Int) {
  def this(s: String) = this(s.length)
  def viaSecondary: Sec = new Sec("abcd")
}
```

Real scalac 2.13.16 prints `4`. An unmodified build of the branch point, in
**both** linking modes, is

```text
java.lang.VerifyError: Bad type on operand stack
  Location: Sec.viaSecondary()LSec; @15: invokespecial
  Reason: Type 'java/lang/Integer' is not assignable to 'java/lang/String'
```

### The descriptor was never wrong

The natural reading of that message -- and the one recorded above -- is that
codegen built the `<init>` descriptor from the class's `ctor_fields` instead
of from the constructor the typer picked. It does not. `javap` on the branch
point's own output says so:

```text
0: new Sec
3: dup
4: ldc           "abcd"
6: checkcast     java/lang/Integer      <-- 
9: invokevirtual java/lang/Integer.intValue:()I   <-- 
12: invokestatic java/lang/Integer.valueOf:(I)Ljava/lang/Integer;  <-- 
15: invokespecial Sec."<init>":(Ljava/lang/String;)V
```

The `invokespecial` names the **secondary's** descriptor, which is exactly
what scalac emits. `gen_new` already receives the picked constructor as
`tree.sym` and `method_desc_from_sym` already renders it. What is wrong is the
three instructions in front of it: the `String` literal is unboxed to `int`
and boxed back, so an `Integer` arrives in a slot the descriptor declares
`String`.

### Where it came from: `erasure::method_param_types`

Erasure asks "what parameter types does this application adapt its arguments
to?" and, for a `new`, answered by taking the class's **first** `<init>`
member:

```rust
if matches!(&fun.kind, TreeKind::New { .. }) {
    …
    for m in &st.get(c).members {
        if st.get(*m).name == "<init>" {
            if let Type::Method { paramss, .. } = &st.get(*m).ty {
                return paramss.iter().flatten().cloned().collect();
            }
        }
    }
```

That is always the primary. So `box_adaptation(String, expected = Int)`
returned `Unbox(Int)` and `wrap_unbox` put an `$unbox` node around the
argument -- while the backend, reading the resolved symbol, emitted the
secondary's descriptor. The two halves disagreed because they were reading
different constructors.

The fix hands `method_param_types` the `Apply`'s own symbol -- the alternative
`pick_ctor_at` chose, and the same symbol `gen_new` builds the descriptor
from -- so they cannot disagree again. Where there is no resolved symbol the
fallback now prefers an `<init>` whose arity matches the call before falling
back to the first, which is what it did unconditionally before.

### The other direction, and the corpus test it turns green

The reduction has the primary taking a primitive and the secondary a
reference. The inverse is the same defect and it is the one the corpus was
already failing on: `test/files/run/kmpSliceSearch.scala` opens with

```scala
val rng = new scala.util.Random(java.lang.Integer.parseInt("kmp", 36))
```

`scala.util.Random`'s primary is `(self: java.util.Random)` -- a reference --
and the constructor this picks is `def this(seed: Int)`. Adapting against the
primary made `box_adaptation(got = Int, expected = java.util.Random)` return
`Box`, so the `int` was boxed and handed to a descriptor that says `int`:

```text
VerifyError: Bad type on operand stack
  Type 'java/lang/Integer' is not assignable to integer
```

`run/kmpSliceSearch` goes `fail` -> `pass` and now matches its `.check` file
exactly. The entire class-file difference is **one instruction**, on a
normalised `javap -c` diff of `Test$`:

```text
88d87
<   invokestatic  // Method java/lang/Integer.valueOf:(I)Ljava/lang/Integer;
```

Two things follow. First, the defect is **not confined to classes declared in
source** -- `Random` is read from the jar, and the constructor alternatives
come out of its pickle. Second, a jar test is not automatically a test for
this: `scala.collection.mutable.StringBuilder`, the obvious candidate, has
nothing but reference parameters on every alternative, so it passed on the
branch point too. `crates/cli/tests/secondaryctor.rs` carries both.

### Yield: one corpus `run` test, zero class files of slick

**slick's 1490 class files are byte-identical** between the branch point and
this change (`SLICK_OUT=… tests/slick_measure.sh` on both binaries, `diff -r`,
exit 0). `errors=0 files_with_errors=0 classes=1490` before and after. The
compile counts do not move either: scala library `917 / 146`, gitbucket
`270 / 79`, cats `185 / 71`, `MODE=b tests/slick_run.sh` `progs=12 ok=12
diff=0 fail=0 attempts=36/36`, `tests/slick_subset.sh` `verified=1490 failed=0
lint_problems=0`. This is a run-time correctness fix, so that is the expected
shape: the corpus `run` population is the only place it could show, and it
did, once.

That is not a vacuous negative -- slick has exactly four secondary
constructors, and each was checked:

| site | primary | secondary | why it cannot move |
|---|---|---|---|
| `RelationalProfile.Table` | `(Tag, Option[String], String)` | `(Tag, String)` | arg 1 adapts to `Option[String]` instead of `String`; both erase to references, so `box_adaptation` returns `None` either way |
| `util.ConstArray` | `(Array[Any], Int)` | `(Array[Any])` | the only argument is parameter 0 of both; identical |
| `jdbc.DriverDataSource` | 8 params | `()` | no arguments, so the adaptation loop is empty |
| `compiler.CompilerState` | `(QueryCompiler, SymbolNamer, Node, HashMap, Boolean)` | `(QueryCompiler, Node)` | arg 1 adapts to `SymbolNamer` instead of `Node`; both references. The primary's primitive `Boolean` is parameter 4 and a two-argument call never reaches it |

`box_adaptation` only produces a `Box` / `Unbox` / `VcBox` / `VcUnbox` when the
two types straddle the primitive/reference line or a value class. Every slick
secondary differs from its primary only in reference positions, so the wrong
answer and the right answer erased to the same tree. The defect needs a
primitive (or a value class) on one side and a reference on the other -- which
is precisely the reduction, and precisely the fixture.

### What it is worth, measured by running it

`tests/fixtures/secondaryctor_new.scala` holds, in one program: a secondary
constructor called from inside the class, from the companion and from an
unrelated object; two secondaries whose erased descriptors differ in exactly
one parameter; a secondary that delegates to another secondary rather than to
the primary; a value class in a secondary's parameter list; a default
argument; and a plain `new C(primary args)` for every one of those classes, so
the primary path is pinned in the same program. The expected output is what
`/tmp/scala-2.13.16/bin/scalac` prints compiling that same file, and this
branch matches it in both `--scala-library` and `--no-scala-library` mode. On
the branch point it is a `VerifyError` in both modes.

The multiset of `<init>` descriptors `Main$` emits is identical to scalac's,
including `Dist."<init>":(I)V` for the secondary that takes a value class.

### Found here, not fixed here

A default argument on a **secondary** constructor is rejected outright:

```scala
class Deft(val p: Int, val q: String) {
  def this(p: String, q: String = "dq") = this(p.length, q)
}
```

`error: value <init>$default$2 is not a member of Main$`.
`Typer::synthesize_ctor_default_getters` only ever runs over the *primary*
constructor's parameters, so the getter the call site needs is never declared
and the lookup falls out to the enclosing object. This reproduces unchanged on
the branch point, it is a hard error rather than a silent miscompile, and it is
a different mechanism from this slice's defect, so it is recorded rather than
fixed. `crates/cli/tests/secondaryctor.rs` asserts the *rejection*, so the test
fails and says so when a later slice implements it; the fixture puts its
default on the primary instead.

**Closed by `agent/ctorgaps` below**, which found a second half to the
diagnosis: the reason the message names `Main$` is that the *ordinary* method
path had declared an instance `<init>$default$2` on the class, and
`default_getter_apply` went looking for a receiver. That slice also found --
and fixed -- the miscompile the fix would otherwise have unlocked: a default
that names a field of the class being constructed read it off the caller's own
`this`. The assertion here is now on the value the default produces.

## The `agent/arrayelem` slice: `new Array(n)` reads its element from `pt`

**917 errors in 146 files → 875 in 146** at the branch point (`e76b0ebf`).
42 removed, **none added** -- the two error sets differ only by deletions,
checked line by line and not by count. Every other target is unchanged to the
error (gitbucket 270 / 79, cats 185 / 71, slick `errors=0 classes=1490`), and
slick's 1490 class files are **byte-identical** (`SLICK_OUT` on the pre-fix
and post-fix binaries, `diff -r` empty).

Merged with `main` at `fd65f6f7` (`agent/nameamb`, `agent/intrinsicqual`,
`agent/basetypemeet`), the same **-42**: `main`'s 912 / 145 becomes **870 /
145**, so this wave and `agent/basetypemeet`'s -5 do not overlap. cats and
gitbucket are `main`'s own numbers there (182 / 71 and 270 / 79), and slick's
1490 class files are byte-identical against that `main` too -- re-checked on
the merged tree, not carried over from the branch point.

Not one `new Array(` is left in the library log.

### The element is the allocation, not a type argument

`new Array(0)` was `no matching overload for constructor Array` in jar mode
and `found: Array[Nothing]` against the library's own sources, and it is
tempting to file that with the other inference gaps. It is not one of them:
`Array` erases to a `newarray`/`anewarray` of a *specific* JVM type, so the
element decides which array class is allocated. Getting it wrong is an
`ArrayStoreException` at run time and **not a compile error** -- an
`Array[Nothing]` is an `Object[]` and will happily accept a `checkcast` to
`String[]` that fails only at the first store.

So the fixtures run rather than compile. `arrayelem` builds one array per
shape the library writes -- a `val` with an ascription, an argument position,
an assignment, a nested element, a constructor argument -- stores into each,
reads back, and prints `getClass.getName`; the expected output is real scalac
2.13.16's, and the emitted `newarray int` / `anewarray java/lang/String` /
`anewarray "[Ljava/lang/Object;"` match its bytecode instruction for
instruction.

### What nsc actually does, measured rather than recalled

Four facts, each from a scalac 2.13.16 run:

* `val a: Array[Int] = new Array(3)` → `newarray int`, and
  `take(new Array(2))` at `(Array[String])Int` → `anewarray
  java/lang/String`. The element comes from the expected type and the
  allocation is direct.
* `def mk[K](): Cell[K] = new Cell[K](new Array(0))` → **`cannot find class
  tag for element type K`**. The element is read off the expected type
  *first*, and only then does the `ClassTag` requirement apply to it. This is
  the negative case, and it is what tells you the inference happened.
* `val a = new Array(3)`, with nothing at all to read from, **compiles**. nsc
  solves the element to `Nothing` and builds it as
  `ClassTag.Nothing.newArray(3)`, whose runtime class is `[Ljava.lang.Object;`
  -- so "nothing to infer from" is not the negative case. scala-rs reaches the
  same array class the other way, by the `anewarray java/lang/Object` that
  `jvm_desc_array_elem` already gives `Nothing`, which is also what scalac
  itself emits for a written `new Array[Nothing](3)`.
* `new Array[Int](10, 10)` and `new Array[Int]` are both rejected on arity,
  and nsc names `Array[T]` when the element was inferred and `Array[Int]` when
  it was written -- the arity check runs *before* instantiation.

### Three things had to be true at once

1. **`SymbolTable::is_array_class`, not `array_sym`.** When the library's own
   `src/library/scala/Array.scala` is under compilation,
   `shadow_supplied_by_source` puts the source class into every scope and
   deliberately leaves `array_sym` pointing at the prelude's symbol, because
   that id is written into prelude signatures still in use. A rule that asked
   about `array_sym` alone was correct against the jar and answered "no" for
   all 64 of the library's own. This is why the first fix measured **917 →
   917**.
2. **The prefix carries the type, not just the node.** `gen_new` reads
   `tpt.ty` to choose between `newarray` and `new`. Writing the element only
   onto the `New` node compiled and then emitted `new "[java/lang/Object"`
   followed by an `invokespecial` of a constructor no array class has.
3. **An argument position has to ask twice.** nsc leaves the constructor's `T`
   undetermined until the argument is checked against the parameter; this
   compiler solves it at the `New`, so `new CNode[K, V](0, new Array(0), gen)`
   (`concurrent/TrieMap.scala`) needed a re-type once the parameter type was
   known (`Typer::array_new_wants`). Two library sites, and the shape the
   brief singled out.

   The re-type must be **monotone**: it keeps an element an earlier pass
   already found. A block is typed once for its value and once for its
   statements, and reading `pt` unconditionally on the second pass overwrote
   the `AnyRef` that `a1 = new Array(WIDTH)` (`immutable/Vector.scala`) had
   just been given. That version measured **895** -- worse than the 877 the
   step before it -- and the 18 it put back were all in `Vector.scala`.

### The arity hole the corpus found

`neg/multi-array` went from pass to fail, and it was the slice's own doing —
but the defect was older. The `new Array…` path typed every argument as an
`Int` and never counted them, so `new Array[Int](10, 10)` was accepted
outright on the *pre-fix* binary too. It looked fine only because the
un-annotated `new Array(10, 10)` was rejected further down for matching no
constructor, which is a different diagnosis and stopped being reached once the
element could be inferred. `new Array[Int]`, with no argument list at all,
never became an `Apply` and had the same hole one node up. Both are now
nsc's own wording, `Array[T]` / `Array[Int]` distinction included.

Worth noting for whoever audits the next `neg` regression: the loss the gate
reported was **not** the audited `neg/name-lookup-stable`, which does not
appear against `corpus-3fd80269.tsv` at all. Check the list, not the count.

### The head after this wave (875)

`type mismatch` 393, `no matching overload` 147, `X is not a member of Y` 143,
`no matching overload for constructor` 31, `not found: value` 27, `ambiguous
overload` 18, `illegal inheritance` 11, `incompatible type in overriding` 9.
The `is not a member of` receivers are now `CC` (18), `T2` (17), `Array` (17),
`String` (10), `T1` (9) -- the prelude collision named at the top of this
file, unmoved. The largest `type mismatch` shapes are `found: T required: A`
(13, the `Iterator.empty.next()` item already on the list), `found: null
required: A` (8), and `found: Array[AnyRef] required: Array[Any]` /
`found: Array[A] required: Array[Any]` (6 each) -- those last two are
**variance**, not construction: `Array` is invariant and the receiver these
appear on is `ArrayOps`, so they are a separate question from this slice's.

## The `agent/hkbound` slice: an applied abstract constructor is at least its bound

**852 errors in 145 files -> 807 in 142.** The brief handed this slice one
cluster -- `CC` was the largest `type mismatch` found-type at 36 -- and asked
whether the two families inside it are one root. **They are two**, and the
measurement says so: the first fix takes 852 to **834** and leaves the second
family untouched at its full size; the second takes 834 to **807**.

cats **182 -> 182**, gitbucket **270 -> 270**, slick `errors=0
files_with_errors=0 classes=1490` with all 1490 class files **byte-identical**
(`SLICK_OUT` on both saved binaries, `diff -r` empty). Both A/Bs compare two
saved binaries (`SCALA_RS=<binary>`), never a revert in the working tree.

### One: the bound of an applied type *parameter* was never read

`BuildFrom.scala` declares

```scala
implicit def buildFromMapOps[CC[X, Y] <: Map[X, Y] with MapOps[X, Y, CC, _], K0, V0, K, V] = ...
  def newBuilder(from: CC[K0, V0]) = (from: MapOps[K0, V0, CC, _]).mapFactory.newBuilder[K, V]
```

-- an ascription of a value to **exactly its own parameter's bound**, which
therefore holds by the bound and by nothing else. `SortedMapOps` writes the
same shape twice more.

`SymbolTable::is_sub_type`'s `(Applied, other)` arm reduced an applied abstract
type *member* to `bound_hi`, substituted at the application's own arguments --
the rule `type CT[T] <: TT[T]` applied to `U` is bounded by `TT[U]`, which an
earlier slice added for slick. It did not reduce an applied higher-kinded type
*parameter* at all, so `CC[K0, V0]` conformed to nothing and fell out of the
arm as `false`.

nsc has no such split. `isHKSubType` falls through to `isSubType2`, whose
abstract-type case reads `sym.info.bounds.hi` for any abstract symbol whatever
kind of symbol it is; a higher-kinded parameter's bounds live inside its
`PolyType` and come out applied. One pattern -- `TypeMember(id) |
TypeParam(id)` -- is the whole change, plus an `enter_bound` guard the member
arm did not have and now shares: `CC[X, Y] <: MapOps[X, Y, CC, _]` mentions
`CC` again, and every other bound arm in the function is guarded for exactly
that reason.

**-18 errors.** `CC` as a `type mismatch` found-type went 36 -> 26; the
remaining 26 were all the second family.

### Two: `@uncheckedVariance` was reachable everywhere except here

`Factory.scala`'s `fill`/`tabulate` ladder is five levels of

```scala
def fill[A](n1: Int, n2: Int)(elem: => A): CC[CC[A] @uncheckedVariance] = fill(n1)(fill(n2)(elem))
```

and it reported `found: CC[CC[A]] required: CC[CC[A] @uncheckedVariance]`, the
reverse, and versions with the annotation at a *different depth* on each side.

`agent/basetypemeet` established that **`@uncheckedVariance` is a spelling, not
a variant**, and folds annotation-only differences when merging base type
arguments. Conformance already had the rule too -- `(a, Annotated) =>
is_sub_type(a, tpe)` and its mirror -- but the two arms sat **below** the
`Applied` arms. Those match on *one* side and every `other`, so
`CC[A] <: CC[A] @uncheckedVariance` hit `(Applied, other)` and
`CC[A] @uncheckedVariance <: CC[A]` hit `(other, Applied)`, and neither ever
reached the rule. It was a question of arm order, not a design question: nsc
strips both sides in `firstTry`, ahead of every `TypeRef` case.

The arms moved **above** the `Applied` arms and **below** the ones that name
`Annotated` in a pattern list. That second half is load-bearing in the other
direction: `Null <: T @ann` and `T @ann <: AnyRef` are answered by those lists
for every `T`, value classes included, and stripping first would turn
`Null <: Int @ann` from true into false. Moving the arms to the very top of the
function -- which is the obvious reading of "nsc strips both sides first" --
would have made that change silently.

**-27 errors**, and `CC` as a found-type is down to 2.

### It costs nothing measurable

Min of five interleaved `src/library` runs, user CPU: **1.78 s before, 1.78 s
after**, spread 1.78-1.83 on either side, on a machine with four other slices
measuring. `agent/basetypemeet` cost about 10% for its rule because
`base_type_args` runs on every `subst_as_seen_from`; this one does not, for two
reasons. The bound read only fires where the arm previously returned `false`,
so it is on the *rejection* path and not the acceptance path, and it terminates
in one substitution. The annotation move adds two discriminant compares ahead
of the `Applied` arms and removes them from further down.

### The silent wrong answer next door, which this does *not* fix

`tests/fixtures/hkbound_appliedbound.scala` writes its type arguments out. With
them inferred, this still happens, on both binaries:

```scala
def pick[K0, V0, CC[X, Y] <: MapOps[X, Y, CC, _]](x: MapOps[K0, V0, CC, _]): String = "pick:ops"
def pick(x: Any): String = "pick:any"
def choose[K0, V0, CC[X, Y] <: MapOps[X, Y, CC, _]](from: CC[K0, V0]): String = pick(from)
```

`choose` compiles and runs **`pick:any`**; scalac 2.13.16 runs `pick:ops`. It
is unchanged by this slice -- measured before and after -- and it is a
different mechanism: solving `?CC` from an actual `CC[K0, V0]` against a formal
`MapOps[?K0, ?V0, ?CC, _]` means reading the *base type arguments* of a type
parameter at its bound, which `base_type_args` does not do. Conformance now
answers the question when the arguments are given; inference still cannot ask
it. Whoever takes that next should expect it to be the same size as the
`readTag`-shaped lines in the log rather than the `CC` cluster, which is gone.

### The fixtures

`hkbound_appliedbound.scala` prints six lines and every one is checked by its
value, in both modes; `expected/hkbound_appliedbound.txt` is real scalac
2.13.16's own run of the same source, and `scalac_agrees_hkbound_runs` compiles
and runs both compilers' output and compares them. On the pre-fix binary five
of the six do not compile.

`hkbound_appliedbound_bad.scala` is the restriction, and all four lines were
rejected before the fix as well -- it is what stops the new arms over-reaching,
not evidence that they exist. The bound is `MapOps` and a `Sink` is not it; two
F-bounded parameters with the same bound *shape* are still two different
constructors (`CC[Int, String]` is not a `DD[Int, String]`); the reduction is
one-way (a `MapOps[K0, V0, CC, _]` is not a `CC[K0, V0]`); and erasing an
annotation does not erase the type under it (`Box[A] @uncheckedVariance` is not
a `Box[String]`). Real scalac rejects all four, at the same lines 24, 31, 37
and 41.

### The head after this slice (807)

`type mismatch` 334, `no matching overload` 178, `X is not a member of Y` 143,
`incompatible type in overriding` 9, `illegal inheritance` 6, `ambiguous
overload` 5. The `is not a member of` receivers are `T2` (17), `String` (10),
`T1` (9), `Int` (6) -- the prelude collision named at the top of this file,
unmoved, and `Array` has dropped out of the head of that list. The largest
`type mismatch` found-types are now `T` (41), `Array` (22), `null` (17) and `A`
(10); the largest single shape is `found: T required: A` (13), the
`Iterator.empty.next()` item already on the list. `found: CC[...]` is **2**
lines, down from 36.

## The `agent/ctorgaps` slice: two constructor gaps, both left reduced by the slices that found them

Two independent defects, taken in one slice because both end at the same
seam -- what a constructor's *pickle* says that its class file cannot.

### 1. A default argument on a secondary constructor

`agent/secondaryctor` recorded this at the end of its section:

```scala
class Deft(val p: Int, val q: String) {
  def this(p: String, q: String = "dq") = this(p.length, q)
}
```

`new Deft("abc")` was `error: value <init>$default$2 is not a member of Main$`.

The diagnosis on record -- "`synthesize_ctor_default_getters` only ever runs
over the primary's parameters" -- is half of it. The other half is why the
message names `Main$` rather than saying the getter is missing:
`check_member::type_def_sig` runs the **ordinary** method path
(`synthesize_default_getters`) for every `def this(...)`, because a secondary
constructor *is* a `DefDef`. That declared an instance method
`<init>$default$2` on `Deft` itself, `default_getter_apply` found it, and --
finding nothing that looked like a static or companion getter -- asked
`default_getter_receiver` for a receiver. `new Deft("abc")` has none, so it
reached for the enclosing object.

So the fix is three pieces, and each one is load-bearing:

* `type_def_sig` no longer calls `synthesize_default_getters` for `<init>`.
  A constructor's getters are not instance methods of the class being
  constructed, which is exactly what `check_template` already says in a
  comment for the primary.
* `check_template` runs `synthesize_ctor_default_getters` over each `def
  this(...)` in the body as well, after the body's signatures are typed. The
  name is nsc's and needed no change: `javap` on scalac 2.13.16's output for
  `class Chain(v: String) { def this(n: Int, sep: String = "-") = … }` shows
  one `$lessinit$greater$default$2`, counted over *that* constructor's own
  flattened parameters. A secondary owes no `apply$default$n` -- a case
  class's synthetic `apply` mirrors the primary alone -- and `Tagged$` in the
  fixture has exactly the members scalac's does.
* `record_secondary_ctor_default_scope`, which is the part that would have
  been a silent miscompile. `record_default_scope`'s existing
  `has_this = false` rule drops **one** scope, because the primary is recorded
  from the template header where the innermost scope is the class's own. A
  `def this(...)` is recorded from `type_def_sig`, which has already pushed
  the constructor's scope on top, so one pop left the class's member scope in
  place. With it there,

  ```scala
  class T(val v: String) {
    val f = "F"
    def this(n: Int, s: String = f) = this(s + n)
  }
  ```

  compiled: `f` resolved to the **field**, and the spliced default read it off
  whatever `this` the caller had. `new T(1)` threw
  `java.lang.ClassCastException: class Main$ cannot be cast to class T`, with
  no diagnostic anywhere. scalac 2.13.16 reports `not found: value f`, and
  this branch now reports it at scalac's line and column
  (`tests/fixtures/ctorgaps_secthis_bad.scala`).

#### One constructor per class may define defaults

A default getter is named after the parameter *position* and nothing else, so
two constructors that both define defaults want the same
`$lessinit$greater$default$2` with different bodies. nsc forbids that for any
overloaded method, constructors included, and reports

```text
in class Two, multiple overloaded alternatives of constructor Two define default arguments.
```

That rejection is implemented here (`check_ctor_default_overloads`), in nsc's
words at nsc's line. It is not decoration: without it the synthesis has to
pick one body for the shared name, and a separately compiled caller of the
other constructor would get the wrong default with nothing to show for it --
the failure mode an error count cannot see. It can only ever refuse programs
scalac also refuses. `tests/fixtures/ctorgaps_secdefault_bad.scala`.

It also closes a corpus test that was not on the brief. **`neg/t7870` goes
`fail` -> `pass`**, and its whole content is this rule:

```scala
class C(a: Int = 0, b: Any) {
  def this(a: Int = 0) = this(???, ???)
}
```

Our diagnostic is `neg/t7870.check`'s, word for word, on its line. An
unmodified build of the branch point compiles it with no diagnostic at all.

#### What it is worth, measured by running it

`tests/fixtures/ctorgaps_secdefault.scala` holds, in one program: a secondary
whose default is omitted and written; a secondary that delegates to *another*
secondary and lets that one's default fill in; a default that is a
*computation* reading a top-level object rather than a literal; a `case class`
whose secondary must **not** grow an `apply$default$n`; a class with defaults
on the primary and a secondary side by side; and a generic class whose getter
repeats the class's type parameters and has its result type inferred. Every
line prints the value the default actually produced -- a getter that answers
with the wrong expression is invisible to an error count.

The expected output is what `/tmp/scala-2.13.16/bin/scalac` prints compiling
that same file, and this branch matches it **in both linking modes**. An
unmodified build of the branch point reports the `<init>$default$2` error five
times over the same file.

**Two of the standard library's 852 errors go away, and none appear.** Both
are `value <init>$default$1 is not a member of ArrayDeque$` --
`scala.collection.mutable.ArrayDeque` has a secondary constructor with a
default. `852 / 145 -> 850 / 145`, and the two error sets differ **only by
those deletions**, checked line by line and not by count. cats (182 / 71) and
gitbucket (270 / 79) are unchanged to the error, diffed as sets.

### 2. Constructor privacy across the class-file round trip

`agent/intrinsicqual` implemented the access check for `private` and
`protected` constructors, closed `neg/sensitive`, `neg/t4987` and
`neg/protected-constructors`, and left `neg/t6601` open because it is a
*separate compilation*.

```scala
// PrivateConstructor_1.scala
class PrivateConstructor private(val s: String) extends AnyVal
// AccessPrivateConstructor_2.scala
class AccessPrivateConstructor {
  new PrivateConstructor("")
}
```

The class file cannot carry the answer. nsc emits even a `private` constructor
`ACC_PUBLIC` -- `javap -p` on scalac 2.13.16's own `PrivateConstructor.class`
says `public PrivateConstructor(java.lang.String)` -- so the `ScalaSignature`
is the only record of it, and the *reading* side was the problem.
`agent/intrinsicqual` had already proved the writing side: real scalac,
reading our class file, accepts `new SepPriv("x")` against the old pickle and
refuses it against the new one.

`PickleSupply::supply_ctors` filtered constructors through
`Member::is_public_api`, which **hides** a private member outright. So the
private `<init>` was dropped from the pickle's contribution, the class file's
`ACC_PUBLIC` one stayed as the only alternative, and the call compiled. It is
now supplied *and marked*: the filter admits any non-bridge, non-synthetic
`<init>`, and `install_ctor` copies `PRIVATE` / `PROTECTED` off the pickled
symbol onto the constructor it repairs. `ctor_access_error`, already written,
does the rest.

`neg/t6601` passes, and it passes for the right reason: our diagnostic is
`neg/t6601.check`'s sentence word for word, on its line.

```text
constructor PrivateConstructor in class PrivateConstructor cannot be accessed
in class AccessPrivateConstructor from class AccessPrivateConstructor
```

`crates/cli/tests/ctorgaps.rs` runs it **twice** -- once reading a class file
this compiler wrote, once reading one real scalac wrote -- because our reader
agreeing with our writer is not evidence that either matches nsc. It also
asserts nsc's `ACC_PUBLIC` premise with `javap`, so the test says why it
passes rather than merely that it does.

#### The risk here is over-rejection, so the ladder runs

Hiding a constructor that is genuinely callable breaks separate compilation
with nothing to catch it, so one separately compiled library carries `private`,
`private[libp]`, `protected`, a public primary beside a private secondary, and
a plain public class; several callers then say what must still compile and
what must now be refused, and the accepted half **runs** and prints. Measured
against the pre-fix binary, one case at a time:

| call, in a separate compilation | before | after | scalac 2.13.16 |
|---|---|---|---|
| `Priv.make("a")` (from the companion) | ok | ok | ok |
| `new Qual("b")` inside `package libp` | ok | ok | ok |
| `new SubProt()`, `class SubProt extends Prot("sub")` | ok | ok | ok |
| `new Mixed("e")` (public primary, private secondary) | ok | ok | ok |
| `new libp.Priv("x")` from another package | **accepted** | refused | refused |
| `new libp.Prot("x")` from another package | **accepted** | refused | refused |
| `new libp.Mixed(5)` from another package | `required: String` | `required: String` | `required: String` |

The last row is nsc's rule that an inaccessible alternative is removed
*before* overload resolution: the private secondary is now an alternative
where it used to be dropped, and the call still has to be a type error against
the surviving `String` constructor rather than an access error.

#### Found here, not fixed here: `private[p]`

nsc pickles a qualified access as the bare `PRIVATE` flag **plus** a
`privateWithin` reference. `crates/pickle` now records that the reference is
there (`Member::private_within`, read from `SymInfo`) but does not resolve
`p`, and `install_ctor` deliberately leaves such a constructor as accessible
as it was before this change. Guessing "private" from the flag alone would
refuse every `private[slick]` constructor slick's own code calls, and
over-rejection is the one failure mode this change must not have. So
`new libp.Qual("x")` from another package compiles where scalac reports
`constructor Qual in class Qual cannot be accessed`. Asserted on the
*acceptance*, so the test fails and says so when a later slice resolves the
boundary.

### Yield: two corpus `neg` tests, two library errors, zero class files of slick

`neg/t6601` and `neg/t7870`, one from each half of the slice, both matching
their `.check` file word for word. The full corpus is `losses=0`; the other
two changes in that run (`pos/t2994a`, `pos/tcpoly_infer_ticket1864`) are
`agent/hkbound`'s, which this branch was merged with before verifying.

**slick's 1490 class files are byte-identical** between the branch point and
this branch (`SLICK_OUT=… tests/slick_measure.sh` on both binaries, `diff -r`,
exit 0). `errors=0 files_with_errors=0 classes=1490` on both. That is the
expected shape and not a vacuous negative:

* slick has four secondary constructors (`RelationalProfile.Table`,
  `util.ConstArray`, `jdbc.DriverDataSource`, `compiler.CompilerState` --
  itemised by `agent/secondaryctor`) and **not one of them takes a default**,
  so the getter synthesis has nothing to add to any of slick's companions.
* slick has four classes with a restricted primary constructor
  (`compiler.CompilerState`, `basic.ConcurrencyControl` and its companion,
  `ConnectionArbiter`) -- the four whose pickled `<init>` flag byte
  `agent/intrinsicqual` moved -- and slick is compiled as **one** run, so
  nothing in it reads its own pickle back. The reading side can only show in a
  separate compilation, which is what `crates/cli/tests/ctorgaps.rs` is.

### Found here, not fixed here

* **A constructor default in a later parameter clause.** `new Curr(7)()` on
  `class Curr(a: Int)(b: String = "b" + a)` emits an `invokespecial` with one
  argument for a two-parameter descriptor: `VerifyError: Bad type on operand
  stack`. It is *not* a secondary-constructor defect -- it reproduces on the
  primary of a plain class, with and without a companion, on an unmodified
  build of the branch point -- and the identical `def m(a: Int)(b: String =
  "b" + a)` is filled correctly. A `new`'s arguments reach
  `fill_defaults_and_implicits` already flattened while that function re-reads
  the callee's *unflattened* `paramss` off the symbol, so the second clause is
  never short. This is the only shape in which a constructor default may
  legally name an earlier parameter (nsc rejects a same-clause reference), so
  it is the one thing the positive fixture cannot cover.
* **Preferring the alternative that needs no default.** nsc fills a default
  only when no alternative applies without one, so `new Prefer(1)` on
  `class Prefer(n: Int) { def this(k: Int, bump: Int = 5) = … }` is the
  primary. Both are applicable here at once: `ambiguous overload for
  constructor`. Pre-existing and a rejection, not a wrong pick.
* **`private[p]` on a constructor**, above.

All three are pinned by assertions on the current behaviour, so a later slice
that closes one is told by a failing test.

## Running it

```
SCALALIB_LOG=$MYDIR/measure.txt SCALALIB_RUN=$MYDIR/run \
  tests/scalalib_measure.sh -no-specialization
```

`SCALALIB_LOG` defaults to a shared path; point it at one of your own.
`SCALALIB_MODE=jar` switches to `--scala-library`, and `SCALALIB_DIRS` picks a
different source set. The script clones scala/scala at the pinned revision and
rebuilds the Java classpath whenever either is missing.
