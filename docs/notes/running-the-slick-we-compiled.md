# Running the slick we compiled (`agent/slickrun`)

`slick_measure.sh` counts type errors. `slick_subset.sh` loads every emitted
class file. Neither runs a single instruction of slick, and — as this slice
found out — the second one does not even verify method bodies: it loads each
class with `Class.forName(initialize = false)`, which links nothing. Its number
means *loads*, not *verifies*. A method whose stack map is inconsistent passes
it without complaint.

`tests/slick_run.sh` is the harness for the next question. It

1. builds slick with scala-rs (`$DIR/out-rs`) and with real scalac 2.13.16
   (`$DIR/out-scalac`, kept and reused — it is the slow half),
2. compiles the twelve client programs in `tests/slick_progs/` **once** with
   real scalac, and
3. runs that one client binary twice, with `out-rs` and with `out-scalac` on the
   classpath, comparing stdout byte for byte.

The client binary is identical in both runs, so every difference is slick's
class files, i.e. scala-rs. `REUSE_RS=1` / `REUSE_SCALAC=0` control the two
builds; `MODE=a` puts the scala-rs build on the client programs' *compile*
classpath as well.

The programs are ordinary slick usage against an in-memory H2 — tables,
`schema.create`, `+=` / `++=`, `filter` / `map` / `sortBy` / `take` / `groupBy`
/ inner and outer joins, `for` comprehensions, `Option` columns, aggregates,
update, delete, `transactionally`, plain SQL, `MappedColumnType`, `Compiled`,
`<>` and `mapTo` — and each prints the generated SQL (`.result.statements`) as
well as the rows, so a miscompiled query compiler shows up as a different SQL
string before it shows up as a different answer.

Note that this revision of slick is the cats-effect rewrite: there is no
`Database.forURL`, no `Future`, no `Await`. The idiom is
`DatabaseConfig.forURL(H2Profile, url, driver = …)` plus `slick.cats.Database`,
whose `run` returns `IO`, so the programs use `.unsafeRunSync()`.

## What it found

Eleven defects, all of them run-time failures of code that type-checked and
whose class files loaded. In the order the harness reached them:

1. **A `match` case's binders counted as free variables of the enclosing nested
   `def`.** `anon_capture`'s `each_child` stopped at `Bind` / `Star` /
   `Alternative`, and `lambda_lift`'s `collect_captures` never bound them at
   all. The lifted method grew a parameter per binder and the enclosing trait
   declared capture accessors no class could implement — the backend filled
   them with `throw new RuntimeException("cannot capture cons for trait
   JdbcProfile")`.

2. **An existential's skolem must erase to its upper bound** (SLS 3.7), like a
   type parameter. slick's `lazy val shaped: ShapedValue[? <: E, _]` makes
   `shaped.value` a `? <: E`; erasing it to `Object` left
   `def baseTableRow: E = shaped.value` without the `checkcast` its own
   descriptor promises, and the verifier rejected the method. That is every
   `TableQuery[…]`, i.e. every slick program's first line.

3. **A signature-pass completion that produced `<error>` was cached forever.**
   Only `<notype>` was undone and retried on the body pass.
   `JdbcCapabilities.insertOrUpdate` is forced early by
   `JdbcActionComponent`'s inferred `lazy val useServerSideUpsert`, came out
   `<error>`, and the cached result left the `val` typed `Object` with no
   accessor and a `throw new RuntimeException("unresolved apply")` in
   `JdbcCapabilities$.<init>`. **File-order dependent**: compiling
   `JdbcCapabilities.scala` first hid it completely, which is how it was
   isolated.

4. **A trait `val` overridden by a narrower one in a derived trait** needs the
   base trait's mixin setter (nsc emits it as a no-op so the base `$init$`
   cannot clobber the override) and a covariant getter bridge.
   `emit_trait_val_accessors` deduplicated by name and emitted neither, so
   `RelationalTableComponent.$init$` hit `AbstractMethodError` on
   `H2Profile$`.

5. **A case class's companion ran the `$init$` of the traits the *class* mixes
   in.** `emit_case_companion` passed the case class where the mixin owner
   belongs: `slick.ast.Apply$.<init>` called `Node$class.$init$(this)` and the
   JVM answered `IncompatibleClassChangeError`.

6. **`3.compare(4)` had no `RichInt.compare` to find**, fell through to the
   `Ordered.orderingToOrdered` view, and the conversion was never materialised:
   `checkcast scala/math/Ordered` landed on an `int`. Declared on the numeric
   `Rich*` wrappers (`crates/typer/src/prelude_richcmp.rs`) and emitted as the
   matching `java.lang.X.compare` static, which is what `OrderedProxy.compare`
   computes anyway.

7. **A `private` constructor the companion calls must lose `ACC_PRIVATE`.** A
   constructor cannot be renamed, so nsc's `makeNotPrivate` is all there is:
   `class L private (…)` with a companion that calls `new L(…)` comes out
   `public`, the same class without such a caller stays `private`.
   `ConstArray$` could not reach `ConstArray.<init>`.

8. **Covariant-override bridges only looked at a class's own members**, so a
   member overridden two traits up got none — `AbstractMethodError` on
   `H2Profile$.MappedColumnType()`, where `JdbcProfile` narrows
   `RelationalTypesComponent`'s `lazy val MappedColumnType`. A trait `val` is an
   interface method too, so the pass covers both.

9. **The mixin `lazy val` accessor held its monitor when the initialiser
   threw.** HotSpot then reports the unbalanced lock and the real exception is
   lost; nsc wraps the region in a catch-all that unlocks and rethrows, and the
   local-`lazy val` accessor in scala-rs already did.

10. **Four erasure/stack holes in `gen.rs`:** the result cast after
    `FunctionN.apply` named only `Class` and `Function`, so a *tuple* result
    went uncast; `if (c) e` with no `else` and a non-`Unit` recorded type left
    the two paths at different stack heights; an auxiliary constructor of an
    inner class had no `$outer` parameter, so a client compiled against nsc's
    ABI got `NoSuchMethodError` on the first `Table` subclass; and a
    user-written `unapplySeq` got its `instanceof` but not its `checkcast`.

11. **Default arguments.** A trait method's `name$default$n` getter is a
    synthesized *symbol*, not a tree, so nothing declared it on the interface or
    implemented it anywhere (`NoSuchMethodError:
    Node.mapChildren$default$2`); a value class's needed an `$extension` static
    as well. And a `case class` pattern naming only the first of several
    parameter lists took the extractor path to a companion `unapply` that is a
    symbol with no method behind it — it now reads the constructor fields, as
    the `Apply` form of the same pattern already did.

The regression fixture is `tests/fixtures/slickrun.scala` (one file, all cases)
with `crates/cli/tests/slickrun.rs`; the expectation is real scalac 2.13.16's own
stdout for the same file.

## Two more, found while porting onto the `invokedynamic` main

* **A `this` inside a hoisted lambda body was pushed twice.** Merging this
  slice's `load_this` with `agent/indy`'s dropped the early `return` from the
  `outer_slot` branch, so the trailing `aload(0)` ran as well. It showed up
  twice, and neither symptom was about pattern-matching lambdas:
  `cases.find { case (f, n) => f(param) }` called `Function1.apply` with the
  extra `this` as the receiver (`IncompatibleClassChangeError`), and
  `byname_lazy`'s `$anonfun$23` had a `Main$Config` where a `StringBuilder`
  belonged (`VerifyError`).

* **`asInstanceOf` on a `<notype>` qualifier materialised a `BoxedUnit`.**
  `adapt_unit_arg` treats `NoType` as `Unit`, which is right for a parameter and
  wrong for a qualifier: there `NoType` means the typer recorded nothing and
  `gen_expr` has already left the real value on the stack. slick's
  `ScalaBaseType.scalaOrderingFor` returns a lambda whose parameters come out
  `<notype>`, and `x.asInstanceOf[AnyRef] eq null` compared `BoxedUnit.UNIT`
  against `null` while `x` stayed stranded. Verified byte-identical on plain
  main before fixing — this one is not a port regression, and it is what
  `slick_subset.sh` cannot see.

## Where it stops now, and why

`ExpandTables` throws

```
ClassCastException: scala.collection.immutable.$colon$colon
  cannot be cast to scala.collection.immutable.IndexedSeq
```

from `tree.collect[…]{…}.toSeq.groupBy(_._1).transform(…)`. `ConstArray.toSeq`
returns `new immutable.IndexedSeq[T] { def apply(i) = …; def length = … }`, so
the groups should be `Vector`s.

The mechanism is measured, not guessed. With a probe compiled by scalac against
each slick build:

| | scalac's slick | scala-rs's slick |
|---|---|---|
| `s.getClass` | `ConstArray$$anon$5` | `ConstArray$$anon$608` |
| `s.iterableFactory.getClass` | `IndexedSeq$` | `IndexedSeq$` |
| `s.groupBy(…)(k).getClass` | `Vector1` | `$colon$colon` |

`iterableFactory` is *right*. The one that is wrong is the erased overload.
`groupBy` uses `newSpecificBuilder`, whose only default in the jar is
`IterableFactoryDefaults.newSpecificBuilder`, and that calls
`iterableFactory()` at the **wide** descriptor `()Lscala/collection/IterableFactory;`.
`immutable.IndexedSeq` overrides `iterableFactory` at the *narrow*
`()Lscala/collection/SeqFactory;`, so the wide call resolves to `IterableOps`'
own default — an `Iterable` factory, whose builder is a `List` builder. The
probe's `s.iterableFactory` reads the narrow descriptor (its static type is
`immutable.IndexedSeq`) and therefore looks correct.

Real scalac closes this by emitting mixin forwarders on the class itself:

```
public scala.collection.SeqFactory<immutable.IndexedSeq> iterableFactory();
public scala.collection.IterableFactory iterableFactory();     // ← the bridge
public mutable.Builder<T, immutable.IndexedSeq<T>> newSpecificBuilder();
public scala.collection.IterableOps fromSpecific(IterableOnce);
public java.lang.Object fromSpecific(IterableOnce);
```

scala-rs emits none of them: its anonymous class declares only `apply` and
`length`. `emit_inherited_covariant_bridges` (added in this slice) cannot help,
because it bridges *to a method the class implements*, and here there is
nothing to forward to but the interface's own default — an `invokespecial` on
the most specific super-interface. Doing that needs the inherited-member set of
the **library** parents, read from their class files rather than from the
prelude, so it is its own slice.

## `MODE=a`: scalac cannot read our pickles for slick yet

Putting the scala-rs build on the client programs' compile classpath makes real
scalac read scala-rs's `ScalaSignature`. It stops at
`value api is not a member of object H2Profile`, so the pickle is not complete
enough for scalac to compile a cake this deep against it. Unaddressed.
