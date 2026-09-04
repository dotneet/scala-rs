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

---

# Second slice (`agent/testkit2`): what a *user* of compiled classfiles needs

## Numbers

Both columns compile the same 56 sources of testkit's stage `main`. They
differ only in where slick comes from.

| | files | errors | files with errors |
|---|---|---|---|
| slick as scala-rs compiled it, before | 56 | 2183 | 51 |
| slick as scala-rs compiled it, after | 56 | **1977** | 51 |
| slick 3.6.1 from Maven, before | 56 | 2534 | 50 |
| slick 3.6.1 from Maven, after | 56 | **2198** | 50 |

`tests/slick_measure.sh` is unchanged at 184 files / 0 errors / 2127 classes.
`crates/backend/` was not touched, so `slick_subset.sh` was not re-run.

The Maven column is worth having because it separates two defects that the
single-column measurement adds together. Class files scalac wrote are known
good, so **every diagnostic in that column is scala-rs's reader or typer**;
class files scala-rs wrote are not, so the first column also carries whatever
our own `ScalaSignature` loses. (Version noise: 3.6.1 is not the checkout, so
the two columns are not comparable to each other, only each to itself.)

## Four roots, all in the reader

Found by pointing scala-rs at the published jar with a twelve-line user of
`slick.jdbc.H2Profile.api`, and reproduced without slick at all in
`tests/fixtures/testkit2_{lib,use}.scala`.

**A nested class had no constructor at all.** nsc writes a top-level class's
`ScalaSignature` once, on that class's own class file; a nested class's file
carries a zero-length `Scala` marker and nothing else (`javap -v
q.Outer$Inner`). A class reached through a type alias -- which is how a slick
profile exports `Table`, `Query` and `Sequence` -- is therefore completed out
of the *enclosing* pickle, and `adopt_binary_class` skips `<init>` by name.
`lookup_member(Table, "<init>")` returned nothing, so
`extends Table[Int](tag, "a")` was "no matching overload for constructor
Table", and a parent in error left the body with no `column`, no `O` and no
`tableTag` behind it. `PickleSupply::supply_ctors` now repairs a class that
has **no** usable constructor from its pickle, installing the source
parameters (the convention the backend's `with_enclosing_outer_param` already
expects) with the real descriptor read out of the class file. It is a repair,
not an addition: a class that already has a readable constructor is untouched.

**A nullary type alias never reached scope.** `expose_unqualified_type` and
`expose_from_wildcards` entered a completion result only when it was a
`Type::TypeMember`, and `install_type_alias` deliberately hands a *nullary*
alias back as its right-hand side with no symbol at all -- giving it one would
make it opaque. `type Tag = lifted.Tag`, and the thirty like it in
`slick.lifted.Aliases`, therefore never entered scope: `class As(tag: Tag)`
had an unresolved `Named` parameter type, and the diagnostic was the
unhelpful "type mismatch; found: Tag required: Tag". Both sites now enter the
alias's *class* when the right-hand side is one.

**`p.x.type` had no reading.** `val O: self.columnOptions.type = columnOptions`
is how `RelationalTableComponent#Table` declares `O`. `conv`'s `Single` arm
handled only a *module's* singleton type, so the member was declined whole and
`O` kept the class file's erased accessor: "value PrimaryKey is not a member
of RelationalTableComponent", 170 diagnostics. `conv_val_widening` now looks
the referent up as a member of its owner and answers with the val's declared
type -- the widening, since there is no singleton type to build, and the
widening is what a selection off the reference reaches.

**An inherited member of a `-cp` class was never completed.**
`complete_on_ancestors` asked only ancestors whose JVM name starts with
`scala/`, and `complete_named` serves a class outside the standard library
only once it has been adopted -- which nothing does for a class the program
merely *inherits*. So `class As(tag: Tag) extends Table[Int](...)` had none of
what `Table` declares. Two paths needed it and neither had it: a selection
through a receiver (`t.describe`) and a bare name inside the subclass body
(`column`, `tableName`), the latter because `enter_inherited_members`
snapshots member lists that are still empty for a jar class. The ancestor walk
now adopts a non-`java.*` ancestor that has a pickle, and
`Check::expose_inherited_from_binary` runs the same completion for an
unqualified name. Together: 695 of the 2534 diagnostics in the Maven column.

## What the measurement now says the blocker is

In the column that the brief tracks -- testkit against **slick as scala-rs
compiled it** -- the remaining top families are no longer reader gaps. The
same forty-line fixture, compiled the other way round, shows why:

```
scala-rs compiles testkit2_lib.scala -> class files
real scalac compiles testkit2_use.scala against them
  error: value api is not a member of object Profile
  error: not found: type Table
  error: value describe is not a member of Main.Users
```

Real scalac cannot read our `ScalaSignature` either, so this is the **writer**
(`crates/backend/src/pickle.rs`), not the reader. Two losses are visible in
that one fixture and account for the largest remaining families:

* **A nested `object` is not declared in the enclosing pickle** -- the same
  shape as the known gap for nested *classes*. `object Profile { object api }`
  loses `api`; in slick this is `H2Profile.api` itself.
* **`val O: Profile.opts.type` comes back as the owner**, so `O.PrimaryKey` is
  "value PrimaryKey is not a member of Profile" -- exactly the
  `RelationalTableComponent` family (170) that the reader fix cleared when the
  class files came from scalac.
* **A secondary constructor is in the bytecode but not in the pickle**, so
  `extends Table[Int](tag, "a")` still fails in that direction.

`tests/fixtures/testkit2_{lib,use}.scala` is the reproduction; today
`crates/cli/tests/testkit2.rs` drives it in the direction that works (scalac
writes, scala-rs reads). Turning the other direction on is the next slice's
acceptance test, and it belongs to whoever owns `crates/backend`.

## Known gaps this did not fix

* **The `$outer` of a super call into a nested `-cp` class.** With the
  constructor repair in place, `class Users(t: Tag) extends P.Table[Int](t)`
  where `P.Table` is nested in a *trait* now typechecks and then emits
  `Table.<init>(Tag, String)` with no enclosing instance:
  `VerifyError: Bad type on operand stack ... uninitializedThis is not
  assignable to Profile`. `gen::with_enclosing_outer_param` prepends the
  *subclass's own* `$outer`, and what this call needs is the prefix of the
  parent type (`H2$.MODULE$`). Nested in an `object` -- which is what the
  committed fixture uses -- there is no outer and the same program runs and
  matches scalac. `crates/backend/src/gen.rs` is another slice's file, so this
  was left alone; testkit emits no class files yet, so nothing bad reaches
  disk today.
* **`X.this.y.type` is rejected in source.** `val O: Profile.this.opts.type`
  is "stable identifier required, but Profile.this.opts found"; the self-type
  alias spelling `self.opts.type` works, which is what slick writes.
* **`TableQuery[E]` is a macro overload.** `TableQuery[As]` picks the
  value-taking `apply` and stays an unapplied method type
  (`((Tag) => As)TableQuery[As]`), so `.schema`, `.result`, `.filter`, `+=`
  and `++=` are all "not a member" -- around 250 diagnostics in the Maven
  column, and the reason so many lambdas there have `<notype>` parameters.
* **An implicit `TypedType[T]` for `column[T]("id")`.** Against the published
  jar scala-rs agrees with scalac exactly (`stringColumnType` does not conform
  to `TypedType[Int]`, `intColumnType` does); against our own class files
  every numeric column type reads as `Any` and every other one matches
  everything, which is the writer again.
