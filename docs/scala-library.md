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

## What to do next, in order

0. **`->` when two conversions offer it — 28 errors, 3 files.** The twelve-line
   reproduction is above. Worth taking before anything else in this list,
   because it is the only regression standing between the current number and a
   clean sweep of the `Predef` work, and because "two candidates, so neither"
   is a wrong answer anywhere it happens, not only here. Note the two fixes
   that look right and are not, recorded above, before starting.
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
4. `src/reflect` and `src/compiler` are not worth measuring yet.
   `SCALALIB_DIRS` accepts them when they are.

## Running it

```
SCALALIB_LOG=$MYDIR/measure.txt SCALALIB_RUN=$MYDIR/run \
  tests/scalalib_measure.sh -no-specialization
```

`SCALALIB_LOG` defaults to a shared path; point it at one of your own.
`SCALALIB_MODE=jar` switches to `--scala-library`, and `SCALALIB_DIRS` picks a
different source set. The script clones scala/scala at the pinned revision and
rebuilds the Java classpath whenever either is missing.
