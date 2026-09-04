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
| **`--no-scala-library`** (default) | 538 | **4226** | **219** | 0 |
| `SCALALIB_MODE=jar` (`--scala-library`) | 538 | 4410 | 221 | 0 |

`classes=0` is expected while errors remain — nothing is emitted — but read the
two together: `errors=0 classes=0` would mean a crash, not a success.

319 of the 538 files draw no diagnostic at all.

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
files to parse took the number to 4226. **That is progress, not a regression.**

The parse failures were five things:

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

## What the 4226 are

| Class | Count | Share |
|---|---|---|
| `X is not a member of Y` | 1391 | 32.9% |
| `type mismatch` | 1154 | 27.3% |
| `no matching overload` | 704 | 16.7% |
| overriding (`overrides nothing`, `override modifier required`, `incompatible type in overriding`, `cannot override final member`) | 263 | 6.2% |
| `not found: value` / `type` / `extractor` | 152 | 3.6% |
| `no matching overload for constructor` | 92 | 2.2% |
| `ambiguous overload` | 58 | 1.4% |
| `object creation impossible` / `needs to be abstract` | ~90 | 2.1% |
| everything else | rest | |

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

## What to do next, in order

1. **Let source definitions replace prelude symbols.** Until a source
   `scala.collection.IterableOnce` *is* the `IterableOnce` the rest of the run
   sees, no further work on this benchmark measures anything: the top three
   error classes are all this. It is not a small change — the prelude is how
   the typer knows the standard library at all — but nothing else is worth
   doing first, and the nine-line reproduction above is the whole test.
2. Only then re-measure and re-classify. The `type mismatch` and `no matching
   overload` pile (1858 errors) is downstream of (1) and cannot be read
   honestly until (1) is fixed; a share of it will simply disappear.
3. Do **not** assume the overriding family (263) is a second root. It looks
   like one — `overrides nothing` does not need member lookup to succeed — but
   the ones sampled are the same bug seen from the other side:
   `Option.scala:171 override final def knownSize` overrides
   `IterableOnce.knownSize`, declared in the source `IterableOnce` that the
   prelude's copy is hiding. There is no evidence yet of a second independent
   root anywhere in these 4226; look for one only after (1).
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
