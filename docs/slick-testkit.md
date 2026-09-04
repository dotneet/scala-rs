# slick-testkit

`tests/slick_measure.sh` compiles slick's own 184 sources. `slick-testkit` is
the layer above: slick's test suite, which is a *user* of slick's API and
reaches it through the classfiles the compiler emitted rather than through
types the typer just computed from source.

## What is measured

`tests/testkit_measure.sh [main|test|all|both]`.

Stage `main` is testkit's compile scope: `slick-testkit/src/main` (48 files)
plus the four modules `build.sbt` puts on its `compile` configuration --
`slick-codegen` (5), `slick-future` (2), `slick-hikaricp` (1) and `slick-zio`
(0 main sources) -- **56 files**. Stage `test` is `slick-testkit/src/test`
(33 files, minus `GeneratedCodeTest.scala`, whose sources sbt generates by
running the code generator against a live H2) and is compiled on top of stage
`main`'s output, so it is blocked until stage `main` compiles. Stage `both`
compiles the two together in one invocation, which is the only way to get a
figure for `src/test` while stage `main` still fails.

The classpath is the directory `slick_measure.sh` left behind (pass it as
`SLICK_CLASSES=<dir>`; the script compiles slick itself when it is not given
one), the shared `deps.cp`, and junit / HikariCP / ZIO, which the script
fetches with `cs fetch` on first use.

`TESTKIT_SLICK_SRC=1` puts slick's *sources* into the same compilation instead
of its classfiles. Same program, different supply path: a diagnostic that
appears only in the classfile configuration is a classfile-emit or
classfile-read bug, not a testkit one.

## Numbers

| | files | errors | of which `not found` | files with errors |
|---|---|---|---|---|
| stage `main`, before | 56 | 2112 | 1478 | 51 |
| stage `main`, after | 56 | 2183 | 52 | 51 |
| stage `both`, after | 90 | 2782 | 119 | 80 |

The total barely moved and that is not the interesting figure. **`not found`
errors went from 1478 to 52.** `import tdb.profile.api.*` -- the shape every
testkit suite is written in -- used to resolve nothing at all, so `column`,
`Table`, `TableQuery`, `Rep`, `O`, `DBIO` and `LiteralColumn` were simply
absent (1478 diagnostics). They now resolve, and what is left is the next
layer down: missing `TypedType` / `BaseColumnType` implicits, `Shape`
resolution, `SchemaDescription` members, overload selection against
`CanBeQueryCondition`. Every name that resolves exposes the errors behind it,
which is why the count did not fall.

The "before" row is main plus the parser fix only. Without it the run stops at
**8 parse errors in 1 file** -- `for (case p <- xs)` in `JdbcMapperTest.scala`
-- and a parse failure aborts before anything is typed, so that 8 was never a
measurement of the typer.

`tests/slick_measure.sh` is unchanged at 184 files / 0 errors / 2127 classes,
and `tests/slick_subset.sh` at `verified=2127 failed=0`.

Running the suite under junit remains out of reach: it needs stage `main` to
compile first. junit, H2 2.4.240, logback, munit and the ZIO test kit are all
fetchable (`cs fetch` works here), so nothing but the compile blocks it.

What stands between here and a compiling testkit, by frequency in the stage
`both` log: an implicit `TypedType[T]` / `BaseColumnType[T]` is not found for
`column[Int]("id")` (104), members of `RelationalTableComponent` (207) and of
`SchemaDescription` (131 + 45), `CanBeQueryCondition` / `Shape` overload
selection where the lambda parameter has no type yet (234), and 111
`override` diagnostics on members that are inherited twice.

## What the measurement found

Four roots. Three are in the compiler; the fourth is that the compiler's own
output cannot be read.

**`for (case p <- xs)`.** Scala 3's spelling of a filtering generator. scalac
2.13.16 accepts the `case` marker with no `-Xsource` flag at all, so the
parser does too, and rejects it before `=` (`case j = e` is an error in nsc).

**A guard on a destructuring generator saw nothing the pattern bound.**
`for ((i, s) <- xs if i > 0)` built its `withFilter` closure from
`pat.name()`, which is `None` for a tuple pattern, so the closure's parameter
was `_` and `i` was "not found: value i". Pre-existing and unrelated to the
`case` marker.

**An import prefix of two or more segments never recovered from a failed
pass.** Imports are typed once per pass and the first pass runs before the
enclosing template's `val`s have signatures. `type_select` retypes a qualifier
only while it is still `NoType`, so a `Select` prefix that failed on pass one
kept its `Error` for the rest of the run -- while the one-segment-shorter
`import d.p._`, whose qualifier is an `Ident` and is always retyped,
recovered on pass four. `import tdb.profile.api.*` is three segments.
The prefix is now cleared before a retry (only while it is unresolved), and a
prefix that later resolves retracts the provisional diagnostics an earlier
pass filed against it.

**Every classfile scala-rs emitted said "extends Object and nothing else".**
`CLASSINFOtpe` was written with `java.lang.Object` as its only parent, so no
inherited member survived into a later compilation. What identified the writer
rather than the reader was pointing **real scalac** at scala-rs's slick
output: it reported the same errors this compiler did
(`value api is not a member of object H2Profile`). Three more losses in the
same signature came out with it:

* a parameterised `type Rep[T] = lifted.Rep[T]` was pickled without its
  parameters ("R does not take type parameters"), which is most of
  `slick.lifted.Aliases`;
* an abstract `type API <: Api` was pickled as `Nothing .. Any`, so a reader
  had none of the members the bound promises;
* `val L = List` named `<root>.List`, and a nested class was referenced -- and
  declared -- through its package, so `slick/jdbc/JdbcProfile$JdbcAPI.class`
  claimed to be `slick.jdbc.JdbcAPI` and no reader looking it up by its real
  name found anything in it.

`crates/cli/tests/testkit.rs` covers all four, including a test that has real
scalac compile against classfiles scala-rs produced.

## Known gaps this did not fix

* **Reading a pickled type alias back.** The writer now emits them; scala-rs's
  own reader still returns a parameterised alias without substituting its
  arguments (`type R[T] = List[T]` used as `R[Int]` comes back `List[A]`) and
  treats a nullary one as opaque. Real scalac reads both correctly from the
  same classfiles, so this is the reader.
* **Nested Scala classes are not declared in the enclosing class's pickle.**
  nsc puts every nested definition in the top-level class's `ScalaSignature`
  and looks them up there. scala-rs writes one pickle per classfile, so nsc
  reports "Symbol 'type slick.jdbc.JdbcActionComponent.MultipleRowsPerStatementSupport'
  is missing from the classpath" when it reads a class that inherits one.
* **No erasure bridge for a `val` whose type is an abstract type member the
  subclass narrows.** `trait DB { type P <: Profile; val p: P }` with
  `type P = P2` in the implementation loads and verifies, then throws
  `AbstractMethodError` at the call. scalac emits a bridge.
