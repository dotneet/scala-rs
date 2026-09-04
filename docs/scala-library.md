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
* **Without** the flag they stay a diagnostic, and that is the right default:
  there is no specialisation phase here, so the class we emitted would silently
  lack the `$mc*$sp` members that everything compiled by real scalac links
  against.
* The rename hole is closed. The parser now remembers selectors that rename
  `scala.specialized` / `scala.annotation.unspecialized`, so `@sp` is diagnosed
  exactly like `@specialized` (`tests/fixtures/scalalib_spec_bad.scala`).

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

**1997 errors in 173 files → 1969 in 172**, both measured on this branch;
`main` had not moved (`1a494fb`), so the merge was empty and the two columns
are the same tree with and without the change. Every other target is unchanged
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

| | before | after |
|---|---|---|
| `tests/slick_measure.sh` | `files=184 errors=0 files_with_errors=0 classes=1596` | identical |
| `tests/slick_run.sh` | — | `progs=12 ok=12 diff=0 fail=0` |
| `tests/cats_measure.sh` | `files=339 skipped=1 errors=2929 files_with_errors=151 classes=0` | identical |
| `tests/gitbucket_measure.sh` | `files=353 skipped=1 errors=1859 files_with_errors=186 classes=0` | identical |
| `tests/scala_corpus.sh` (`CORPUS_SIZE=full`) | `pos 977 · neg 640 · run 434` | identical |
| `cargo test --workspace --release` | — | 149 × `test result: ok`, 1968 tests |

`tests/slick_subset.sh` was not run: this slice touches no code generation
(`crates/backend/` is untouched), so its 30 minutes would measure nothing.

## What to do next, in order

1. **`Vector2[Any]` … `Vector6[Any]` — 100 errors, all in `Vector.scala`.**
   `new VectorN(…)` on a generic constructor infers `Any` for the element
   where the context expects `Vector[B]`. Nothing to do with the prelude; it
   is constructor type inference. `Tree[A, …]` (73, `RedBlackTree.scala`) and
   `Array[Any]` (43) look like the same shape and should be checked together.
2. `case class` synthesis does not produce `canEqual`, so all 22 `TupleN`
   classes report `class TupleN needs to be abstract` against
   `Product`/`Equals`. 22 errors, one root, and it needs no lookup work.
3. Do **not** assume the overriding family (now 51: 30 `` `override` modifier
   required`` plus 21 `incompatible type in overriding`) is a second root. It
   looks like one — `overrides nothing` does not need member lookup to
   succeed — but the ones sampled were the same bug seen from the other side.
   It has shrunk from 263 along with everything else, which is consistent with
   that reading.
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
