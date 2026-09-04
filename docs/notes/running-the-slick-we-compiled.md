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

  **This was a merge artifact of this port, not a hole in the indy lowering.**
  `git show <main>:crates/backend/src/gen.rs` has `load_this` as a clean
  `if / else if / else`; my version used early `return`s, and the textual merge
  kept main's `if / else if` head with my body's trailing `aload(0)` outside the
  chain. Pattern-matching function literals on the invokedynamic path are fine,
  and there is no reason to keep them off it. `slickrun.scala` now carries four
  of them — `find` / `map` / `foreach` over a tuple, one in a trait method, one
  capturing a `var`, one capturing the enclosing `this` — and
  `fixtures_slickrun_pattern_lambdas_are_hoisted` pins that all four become
  hoisted `$anonfun$` statics with no closure class, with the run above
  checking that their binders, captures and outer reference line up.

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

# The bridges (`agent/ifacebridge`)

## What the `ClassCastException` really was, and what it was not

The mechanism above is right, with one correction that matters for the fix:
**nsc's mixin forwarders are `invokestatic` on the interface's `m$` helper,
not `invokespecial` on the most specific super-interface.**

```
public scala.collection.SeqFactory<immutable.IndexedSeq> iterableFactory();
  0: aload_0
  1: invokestatic  InterfaceMethod immutable/IndexedSeq.iterableFactory$:(…)Lscala/collection/SeqFactory;

public scala.collection.IterableFactory iterableFactory();        // the bridge
  0: aload_0
  1: invokevirtual Method iterableFactory:()Lscala/collection/SeqFactory;
```

`invokespecial` would not even be legal for most of them: it requires a
*direct* super-interface, and the interface that implements the member usually
is not one.

Two more corrections to the earlier note:

* nsc emits **about 250** forwarders on `ConstArray$$anon$5`, not thirty.
  `-Xmixin-force-forwarders` defaults to on, so *every* concrete member
  inherited from a trait gets one, whether or not anything depends on it.
* The `ClassCastException` was not the whole of it. On the same anonymous
  class `filter` threw `AbstractMethodError` on
  `fromSpecific(IterableOnce)Object`, and `toString` printed
  `slick.util.ConstArray$$anon$630@281e3708`.

## What this slice emits

`crates/backend/src/ifacebridge.rs` reads the *class files* of a class's
super-types — the symbol table cannot help, because nothing in slick ever names
`iterableFactory` — and emits the narrow subset of nsc's forwarders that
changes behaviour. The other ~240 are pure indirection the JVM's own
default-method resolution already gets right.

1. **Erased overloads.** A `(name, parameter list)` declared with two different
   erased return types along the super-type chain gets the wide spelling on the
   class, forwarding to the narrow one with `invokevirtual` on itself. The
   narrow one is the declaration whose owner is a sub-type of every other
   owner; an ambiguity (two unrelated interfaces) is left alone.
2. **`toString` / `hashCode` / `equals`.** A method inherited from the
   superclass beats an interface default (JVMS 5.4.3.3), and `java.lang.Object`
   is above every class, so a trait's implementation of these three never ran.
   The forwarder is nsc's: `invokestatic <iface>.toString$`.

On `ConstArray$$anon$630` that is eight bridges plus `toString`, against nsc's
250, and the probe below now matches nsc byte for byte:

| `ConstArray.from(List(1,2,3,4)).toSeq` | nsc | before | after |
|---|---|---|---|
| `iterableFactory.getClass` | `IndexedSeq$` | `IndexedSeq$` | `IndexedSeq$` |
| `groupBy(_ % 2)(0).getClass` | `Vector1` | `$colon$colon` | `Vector1` |
| `map(_ + 1).getClass` | `Vector1` | `$colon$colon` | `Vector1` |
| `filter(_ > 2).getClass` | `Vector1` | `AbstractMethodError` | `Vector1` |
| `take(2).getClass` | `Vector1` | — | `Vector1` |
| `toString` | `IndexedSeq(1, 2, 3, 4)` | `…$$anon$630@281e…` | `IndexedSeq(1, 2, 3, 4)` |

The regression test is `crates/cli/tests/ifacebridge.rs`. It cannot use
`scala.collection`: scala-rs does not accept `new immutable.IndexedSeq[T] { … }`
outside a run that also reads the collections from their class files (see
"Still open" below). It builds a stand-in library with real scalac instead
(`tests/fixtures/ifacebridge_lib.scala`), which has the identical class-file
shape — a covariant override with no bridge on the interface, plus a trait
`toString` / `hashCode` / `equals`.

## Four more run-time defects, found by walking the harness forward

Removing the blocker moved `tests/slick_run.sh` four failures deeper. Each of
these was already on `main`; the first two are why the harness no longer
reached `ExpandTables` at all.

1. **`this` in a template's own constructor invocation.** The arguments of
   `new C(this.x) { … }` belong to the *enclosing* expression, so `this` there
   is the enclosing template's `this` — nsc types it with the enclosing class
   as `enclClass`, the same rule `super` already followed here
   (`Checker::super_owner`). scala-rs bound it to the anonymous class's own
   slot 0, which is still uninitialised at that point:

   ```text
   VerifyError: Bad type on operand stack
     Location: slick/util/ClassLoaderUtil$$anon$619.<init>()V @2: invokevirtual
     Reason: Type uninitializedThis is not assignable to 'java/lang/Object'
   ```

   from `new ClassLoader(this.getClass.getClassLoader) { … }` in
   `object ClassLoaderUtil` — the first line of every one of the twelve
   programs. The backend needed the other half: an enclosing `object` is
   reached as its singleton, and an enclosing class through the constructor's
   own `$outer` argument (a `getfield` on `uninitializedThis` is what JVMS
   4.10.1.9 forbids, which is why `ctx.presuper_outer` exists).

2. **A `def this()` that leaves defaulted parameters out.** `type_ctor_delegation`
   resolved the constructor but never filled the defaults in, so the emitted
   `<init>` pushed one argument for a five-parameter `invokespecial`. slick's
   `DriverDataSource` has `def this() = this(null)` in front of eight defaults.

3. **Overloaded concrete trait methods got one mixin forwarder between them.**
   `emit_mixin_forwarders` deduplicated by name. slick's `JdbcBackend` declares
   three `makeDatabase`s and `JdbcBackend$` got a forwarder for whichever came
   first, so `makeDatabase(JdbcDatabaseConfig, Async)` was an
   `AbstractMethodError`. The key is now the name *and the erased parameter
   list* — deliberately not the return type, because a class that narrows an
   inherited member covariantly does override it and the wide descriptor is
   `emit_erasure_bridges`' business.

4. **A `Foo.bar` whose class was stubbed before its class file was read.**
   `gen_ident`'s `SymKind::Class` arm used `Flags::JAVA` to decide whether a
   companion `MODULE$` exists, and `find_or_stub_java_class` allocates every
   stub with that flag — `apply_java_class_meta` only ever *adds* flags, so a
   Scala class stubbed first keeps it for the whole run. `cats.effect.IO` is
   one: `IO.blocking(…)` came out with no receiver at all
   (`Operand stack underflow` in `slick.cats.Database$.$anonfun$1`). The test
   is now the companion's own JVM name — only a Scala companion is this class's
   `Foo$` — which leaves a real Java class alone. Clearing `Flags::JAVA` in
   `apply_java_class_meta` instead does fix it, and it also makes
   `member_module_outer` treat `cats.effect.kernel.Ref$Make$` as an *inner*
   object needing an enclosing instance, which it is not: that flag is load
   bearing there.

## Where it stops now

```
ClassCastException / VerifyError: Bad type on operand stack
  Location: slick/cats/Database$$anon$265.$anonfun$3 @11: checkcast
  Reason: Type integer is not assignable to 'java/lang/Object'
```

`fs2.Stream.fromIterator[IO](it, chunkSize = 1)`.
`fs2.Stream.PartiallyAppliedFromIterator` is a **value class**, so
`fs2/Stream$.fromIterator` really does have the descriptor `()Z`, and nsc
compiles the application as

```
getstatic     fs2/Stream$PartiallyAppliedFromIterator$.MODULE$
getstatic     fs2/Stream$.MODULE$
invokevirtual fs2/Stream$.fromIterator:()Z
…
invokevirtual fs2/Stream$PartiallyAppliedFromIterator$.apply$extension:(ZLscala/collection/Iterator;ILcats/effect/kernel/Sync;)Lfs2/Stream;
```

scala-rs emits `checkcast fs2/Stream$PartiallyAppliedFromIterator` on the
boolean and then `invokevirtual …PartiallyAppliedFromIterator.apply`.
`note_source_value_classes` only sees value classes declared in the run;
one that arrives from `-cp` is not recognised at all. That is the next slice.

## Still open, found on the way

* **`extends` a scala-library collection trait does not compile on its own.**
  `class C extends scala.collection.immutable.IndexedSeq[Int]` (and
  `immutable.Seq`) is `no matching overload for constructor Seq`:
  `inherited_superclass` walks the base type sequence and finds
  `scala/collection/Seq` as a *class*, because that symbol is a
  `find_or_stub_java_class` stub — `Flags::JAVA`, no `INTERFACE` — that nothing
  ever completed from its class file. Inside the full slick run the same
  parents are read properly and the source compiles, which is why slick's
  `ConstArray.toSeq` was never affected.
* **A `-cp` Scala class and its companion object can end up as one symbol.**
  With `sealed abstract class Api[+A]` and `object Api extends ApiPlatform` in
  a scalac-built library on `-cp`, `Api.direct` and `Api.blocking(s)` are
  emitted as `invokevirtual ifb/Api.direct` / `ifb/Api.blocking` with **no
  receiver**: the object's inherited members are attributed to the class.
  Both spellings of the fix in item 4 above leave this one alone, and it is a
  different defect from `cats.effect.IO`'s (whose companion link was intact).

# Value classes from `-cp`, and three erasure rules behind them (`agent/cpvalueclass`)

## The blocker: `extends AnyVal` lives only in the pickle

The diagnosis above named `note_source_value_classes`, and that is only half
of it. The gate it holds never came into play, because
`SymbolTable::is_value_class` was already answering **no** for
`fs2.Stream.PartiallyAppliedFromIterator`. It asks for two things, and a value
class arriving on `-cp` had neither.

* **The `AnyVal` parent.** A value class's class file says
  `extends java/lang/Object` and lists no interfaces — nothing in it
  distinguishes `class Meters(val n: Int) extends AnyVal` from a plain final
  class. The parent survives only in the `ScalaSignature`, and both readers
  dropped it: the eager one (`backend::pickle::unpickle`) read
  `CLASSINFOtpe`'s class symbol and discarded the `{tpe_Ref}` parent list that
  follows, and the lazy one (`pickle_supply::attach_parents`) converted
  `scala.AnyVal` and then skipped it, because it only accepted a
  `Type::Class`.
* **The single constructor field.** It is built from the pickled constructor,
  and only *top-level* class files carry a `ScalaSignature`. A **nested**
  value class — `fs2.Stream.PartiallyAppliedFromIterator`, and every one of
  slick's — is adopted from its class file alone. Its one field is `private`,
  which `parse_java_classfile` drops, so `JavaClass` now keeps the sole
  non-static field whatever its access. That field *is* the class's
  representation, its descriptor is the underlying type, and its name (after
  the last `$$`, for a nested class's expanded name) is the accessor
  `$vcunbox` calls.

`value_class_of` then stops gating on `source_value_classes` and gates on
`prelude_end`, which is what the comment there was really describing: the
exclusion exists for `StringOps` / `ArrayOps` / `RichInt`, which the prelude
models as identity conversions over their underlying value. A value class read
from a class file is not one of those.

Last, the call. An `$extension` on a companion module takes the receiver as
its first *argument*, so the module has to sit under the receiver **and**
under the arguments that follow. The old code pushed it afterwards and
shuffled with `dup_x2; pop`, which is correct for exactly one argument;
`apply$extension(Z, Iterator, I)` came out as `[recv, MODULE$, it, n]`. It is
now pushed before the receiver is evaluated, which is what nsc emits too. The
rule that was spelled "the JVM name contains a `$`" is now named
`value_extension_module`: nsc emits static forwarders on the value class
itself only for a **top-level** one, so a nested one can only be reached
through the module.

This is much wider than fs2. `cats.syntax.FlatMapOps` is a value class too, so
every `>>` in slick's cats-effect layer was being compiled as an instance call
on a `checkcast`ed `Object`.

Regression test: `crates/cli/tests/cpvalueclass.rs`, which builds a stand-in
library with real scalac (`tests/fixtures/cpvalueclass_lib.scala`) — a value
class over a primitive, one over a reference, one that also extends a
universal trait, and the fs2 shape nested in an object — compiles the same
client with both compilers against it, and compares stdout byte for byte.

## Three more, one line of `p01_basic` apart

Removing the blocker moved the harness three times. All three were already on
`main`; none is a regression from the value-class work.

1. **The dominator of a compound type.** SLS 3.7 / nsc's
   `intersectionDominator` is not `parents.head`: it is the first parent that
   is a class rather than a trait *and* that no other parent is a subclass of;
   if none is a class, the first unshadowed one. slick's

   ```scala
   implicit def tableQueryToTableQueryExtensionMethods[T, U](
     q: Query[T, U, Seq] & TableQuery[T]): TableQueryExtensionMethods[T, U]
   ```

   erases its parameter to `TableQuery`, because `TableQuery <: Query` shadows
   `Query`. We wrote `Query`, and the client — compiled by real scalac against
   nsc's slick — got `NoSuchMethodError:
   JdbcProfile$JdbcAPI.tableQueryToTableQueryExtensionMethods(slick.lifted.TableQuery)`
   on `coffees.schema`, the first line of all twelve programs.

2. **A bridge for an inherited member whose *parameter* was narrowed.**
   `emit_inherited_covariant_bridges` bridged covariant *results* only, so
   `H2Profile$` implemented
   `createSchemaActionExtensionMethods(SqlProfile$DDL)` — the type
   `SqlProfile` fixes the abstract `SchemaDescription` to — and nothing at the
   descriptor `RelationalActionComponent` declares. `AbstractMethodError` on
   `schema.create`. nsc's bridge takes the wide descriptor and `checkcast`s
   each narrowed argument.

3. **A lambda parameter still typed as a tuple after erasure** had its
   `checkcast` hard-coded to `scala/Tuple2` — in two places in `gen.rs`.
   slick's
   `Resource.makeCase(acquireStreamContextAndIterator(a)){…}.map(_._2)` cast a
   `Tuple3` parameter to `Tuple2` and then called `Tuple3._2` on it, and the
   verifier threw `BasicBackend$BasicDatabaseDef$class.$anonfun$5` out whole.
   It reproduces in nine lines with no classpath: the arity is lost only when
   the type argument is *written* (`Box.mk[(A, B, C)](…)`), not when it is
   inferred.

`tests/fixtures/erasure3.scala` + `crates/cli/tests/erasure3.rs` check all
three against real scalac 2.13.16 — stdout for what stdout can see, and
`javap` for the two descriptors only a separately compiled caller links
against.

## Where it stops now

`ok=0 diff=0 fail=12` still, but the twelve programs now die in three
different places instead of one, and two of them get real work done first.
`p01_basic` opens the database, compiles and runs `schema.create`, and inserts
both ways:

```text
create table "COFFEES" ("COF_NAME" VARCHAR NOT NULL PRIMARY KEY,"PRICE" DOUBLE NOT NULL)
inserted=1
inserted=Some(2)
```

byte-identical with the scalac-built slick (`p07_caseclass` prints `ins=1` /
`ins=Some(2)` the same way). Then:

1. **Nine of the twelve** — everything that reaches a `.result` —

   ```text
   ClassCastException: class slick.ast.ProductNode cannot be cast to
     class slick.ast.ResultSetMapping
       at slick.ast.ResultSetMapping.withInferredType(ClientSideOp.scala)
       at slick.ast.ResultSetMapping.withInferredType(ClientSideOp.scala)
       at slick.ast.Node$class.infer(Node.scala)
   ```

   (`p07_caseclass` and `p08_mapto` say `TypeMapping` instead of
   `ProductNode`, `p04_groupby` says `Select`.)
   `withInferredType` is declared `def withInferredType(scope: Type.Scope,
   typeChildren: Boolean): Self` on `Node` and refined by each subclass's
   `type Self`; the two `withInferredType` frames say the recursion is going
   through the wrong one, so the cast is ours on a value that is legitimately
   a `ProductNode`. **This is the next blocker.**

2. **`p09_plainsql`** —

   ```text
   VerifyError: Type 'slick/jdbc/ActionBasedSQLInterpolation' is not
     assignable to 'scala/StringContext'
       Location: slick/jdbc/ActionBasedSQLInterpolation.sqlu(…) @2: invokestatic
   ```

   The instance method we emit beside a *source* value class's `$extension`
   statics pushes `this` where the underlying value belongs:
   `aload_0; aload_1; invokestatic sql$extension(StringContext, Seq)`. It
   needs the accessor first. Byte-identical on `main`, so this one is
   independent of the value-class work.

3. **`p10_types`** — `NoSuchMethodError:
   RelationalProfile$ColumnOption$Length$.apply$default$2()`, on
   `O.Length(64)`. A case class's default-argument getter on a *nested*
   companion module.

4. **`p12_mapped`** — `NoSuchMethodError:
   RelationalTypesComponent$MappedColumnTypeFactory.base(…)`, which is
   `agent/slickrun3`'s.

## Still open, found on the way

* **A `-cp` method whose *parameter* is a value class** is installed at its
  erased descriptor, so the call is declined: `def box(m: Meters): Any` on the
  classpath is "no matching overload for (Int)AnyRef with arguments (Meters)".
  The pickled parameter type (`Meters`) is available and is not being
  preferred over the class file's `(I)`.
* **A boxed value class reaching a lambda is not unboxed.**
  `List(new Meters(5), new Meters(6)).map(_.raw)` hands `raw$extension(I)` a
  `Meters`. This one fails identically with the value class declared in
  *source*, on plain `main`, so it is not about `-cp` at all.

# `MappedColumnType`, and the query compiler behind it (`agent/slickrun3`)

## The assigned symptom, and what it really was

`p12_mapped` was the one program that failed differently from the other
eleven. Its first line threw

```text
NoSuchMethodError: slick.ast.TypedType
  RelationalTypesComponent$MappedColumnTypeFactory.base(
    Function1, Function1, ClassTag, slick.ast.TypedType)
```

`javap` on the two builds said scala-rs had declared it
`base(Function1, Function1, ClassTag, Object)Object`. The parameter and the
result are both `BaseColumnType[…]`, and

```scala
type ColumnType[T] <: TypedType[T]
type BaseColumnType[T] <: ColumnType[T] & BaseTypedType[T]
```

is an abstract type member with a **compound** upper bound, which `erase_ty`
answered `Object` for on purpose — the note in the code said guessing at nsc's
`intersectionDominator` had cost the macro bridges their checkcasts.

**That reading of nsc was wrong, and measurably so.** nsc really does pick a
dominator here, and it really does drop the rest of the bound:
`javap` on `scala-reflect.jar` shows `Names.newTermName` returning
`Names$TermNameApi` for a `TermName` declared
`>: Null <: TermNameApi with Name` — and `Names$TermNameApi` is an empty
*interface* that does not extend the abstract *class* `Names$NameApi`. So the
erasure is right and the **call site** is what owes the cast. Both halves are
implemented now:

* `erasure::intersection_dominator` is nsc's rule — a parent that some other
  parent is a strict sub-class of is shadowed; among the rest a non-trait class
  wins, else the first. It compares parents' *symbols*, not their types:
  `SymbolTable::class_sym_of` chases an abstract member to its bound, which
  made `ColumnType[T] with BaseTypedType[T]` look like `BaseTypedType`
  shadowing `ColumnType`, where nsc keeps the latter (and so erases to
  `TypedType`, not `BaseTypedType`). It is used for a `Refined` *type* as well
  as for a compound bound: slick's `implicit def
  tableQueryToTableQueryExtensionMethods(q: Query[T, U, Seq] & TableQuery[T])`
  takes a `TableQuery` in nsc, and taking `parents.first()` gave `Query`.
* `gen::adapt_type_member_arg` now also casts when the argument's *own* erased
  class is not assignable to the parameter's — but only when the parameter's
  class is a real class, since JVMS 4.10.1.2 makes every class type assignable
  to an interface type.

## Six defects between there and the SQL

Each was found by walking a probe forward against both slick builds, and each
was checked against `out-rs` built by the *previous* `main` before being called
new. Two of them (5, 6) were already on `main` and only became reachable here.

1. **The erasure above.**

2. **A subclass that narrows an abstract-typed parameter got no bridge.**
   `MappedJdbcType.base(…, JdbcType)JdbcType` implements
   `…base(…, TypedType)TypedType`, and after erasure nothing says those are one
   method rather than two overloads — `bridge_overrides`' `erases_to_object`
   test only recognises a parameter that erased *to `Object`*, and an abstract
   member with a class bound does not. `SymbolTable::erased_abstract_params`
   now records, per method, which parameters were a type parameter or an
   abstract type member **before** erasure, and `bridge_overrides` reads it.
   Without the bridge the interface method stayed abstract
   (`AbstractMethodError`).

3. **A local's type was re-read through `this`.** `Typer::bind_found` applied
   `expand_type_members(this_class, …)` to every identifier's type. That is
   right for a class member — an inherited `find` is seen through this class —
   and wrong for a local or a parameter, whose type is written in the
   vocabulary of the method that owns it. `Type::TypeMember` carries no prefix,
   so `map.Self` and `this.Self` are the same tree, and the rewrite bound the
   first to the second. slick's

   ```scala
   val (map2, newType) = from2.nodeType match { … }
   ```

   in `ResultSetMapping.withInferredType` then `checkcast`ed a plain `Node` to
   a `ResultSetMapping` — thrown by every query the compiler ran. The guard is
   the same `owner_is_class` test the `subst_as_seen_from` above it already
   used.

4. **A block in statement position asked for its value.** `gen_stat` had no
   `Block` arm, so a block fell through to `gen_expr` + pop — which puts a
   *branching* last expression back in value mode. nsc's `genLoad(block, UNIT)`
   passes UNIT straight down to the last expression instead. slick's

   ```scala
   case '\\' => pos += 1; if (pos < len) { str.charAt(pos) match { … } }
   ```

   in `QueryInterpolator.appendString` generated the inner match for its `Any`
   lub, and only the arms whose own type was not `Unit` left anything on the
   stack: `VerifyError: Inconsistent stackmap frames`.

5. **`withFilter`'s result was replaced by the receiver's type.** The rule
   exists for the collections, where the declared result is the receiver
   *widened* (`Iterable.withFilter` reached through a `List`). slick's
   `ConstArray.withFilter(p): ConstArrayOp[T]` returns something the receiver
   is not, and replacing it made the `foreach` of
   `for ((sym, j: Join) <- from)` resolve to `ConstArray`'s, with a
   `checkcast ConstArray` on the anonymous `ConstArrayOp`. The substitution is
   now only made when the receiver's class conforms to the declared one.

6. **`super.m` landed on a mixin that only re-declares `m`.** nsc resolves
   `super.m` to the first *concrete* `m` along the linearization.
   `super_select_member` took the first parent that had a member of that name
   at all, and slick's `BasicStreamingQueryActionExtensionMethodsImpl` narrows
   `result` covariantly and leaves it abstract — so `JdbcStreamingQuery…Impl
   .result` was emitted as `invokestatic
   BasicStreamingQueryActionExtensionMethodsImpl$class.result`, naming a class
   file that does not exist, because the trait has no concrete member at all
   (`NoClassDefFoundError`).

The regression fixture is `tests/fixtures/slickrun3.scala` (one file, all
cases, expectation taken from real scalac 2.13.16) with
`crates/cli/tests/slickrun3.rs`, which also pins the emitted descriptors so a
change that keeps the stdout by another route has to say so.

## Where it stands

`tests/slick_run.sh` is **0 of 12**, but not for any of the reasons above and
no longer for the same reason for all twelve at once.

On this branch *before* merging `agent/cpvalueclass`, all twelve — `p12`
included — stopped in `Database.make` on the `fs2.Stream.fromIterator` value
class from `-cp`, with one identical `VerifyError`. `p12`'s own defect was
gone: it no longer failed differently from the rest, which was this slice's
assignment.

**With that merge in**, the twelve get considerably further: they open H2,
create their schema, insert rows (`p12` prints `ins=Some(3)`, byte-identical
to the scalac build) and print the generated SQL, then stop at the first
`.result` on

```text
RuntimeException: unresolved apply
  at slick.jdbc.StreamingInvokerAction$class.run
```

which is scala-rs's own marker for a call it could not resolve at compile
time — `createInvoker(statements).foreach(x => b += x)(ctx.session)`. The
curried `iteratorTo(0)(ctx.session)` two lines below it resolves fine, so the
function argument is what distinguishes them. That is the next blocker for all
twelve.

To see past that blocker this slice used a probe with everything but the
database — `MappedColumnType`, `Compiled`, the `Table` definition, and the
statements of eight queries. On `main`'s compiler it dies on its first line
(the `NoSuchMethodError` above). Now it prints, byte for byte, what the
scalac-built slick prints:

```text
create table "BRICKS" ("ID" INTEGER NOT NULL PRIMARY KEY,"C" VARCHAR NOT NULL,"ALT" VARCHAR)
all sql: select "ID", "C", "ALT" from "BRICKS" order by "ID"
byColour sql: select "ID", "C", "ALT" from "BRICKS" where "C" = ? order by "ID"
byRange sql: select "ID", "C", "ALT" from "BRICKS" where ("ID" >= ?) and ("ID" <= ?) order by "ID"
eq sql: select count(1) from "BRICKS" where "C" = 'B'
optCol sql: select "ALT" from "BRICKS" order by "ID"
```

That is slick's query compiler and its whole SQL generator running on class
files scala-rs produced.

## The next one, measured but not fixed

The probe's next line is `bricks.insertStatement`:

```text
AbstractMethodError: slick.jdbc.H2Profile$ does not define or inherit
  RelationalActionComponent.createInsertActionExtensionMethods(Object)
```

It is defect 2 again, one step further out: `H2Profile$` *inherits*
`createInsertActionExtensionMethods(JdbcCompiledInsert)` from
`JdbcActionComponent` and needs the wide `(Object)` bridge for
`RelationalActionComponent`'s declaration, whose parameter is the abstract
`CompiledInsert`. `emit_erasure_bridges` looks at the class's own members;
`emit_inherited_covariant_bridges` covers an inherited member but only bridges
the *return* type. Present on `main` too (checked by disassembling a slick
built with `main`'s compiler).

# The first completed programs (`agent/selfrec`)

`tests/slick_run.sh` went from `ok=0 diff=0 fail=12` to

```text
progs=12 ok=4 diff=2 fail=6
```

`p01_basic`, `p04_groupby`, `p08_mapto` and `p12_mapped` print, byte for byte,
what the scalac-built slick prints — the whole path from `DatabaseConfig.forURL`
through `schema.create`, inserts, the query compiler, the SQL generator, JDBC
execution and result mapping, on class files scala-rs produced. `p02_queries`
and `p06_update_tx` now run to completion too, with output that differs (below).

Six defects. All of them were on `main`; none is a regression from any slice.

## 1. The receiver hoisted for an omitted default wrapped the wrong node

`default_recv::hoist_default_receivers` binds a computed receiver to a local so
a call that omits defaults evaluates it once, and it ran bottom-up on every
`Apply`. A curried call whose default sits in a clause that is **not** the last
one therefore had its *inner* application wrapped, leaving
`Apply { fun: Block { … }, args }` — a callee with no symbol, which `gen_apply`
emits as `throw new RuntimeException("unresolved apply")`.

slick's `StreamingInvokerAction.run` is

```scala
createInvoker(statements).foreach(x => b += x)(ctx.session)
// final def foreach(f: R => Unit, maxRows: Int = 0)(implicit session: …)
```

and that threw in all twelve programs at their first `.result`.

The earlier note guessed the function argument was what distinguished this from
the `iteratorTo(0)(ctx.session)` two lines below. **It is not.** `iteratorTo`
has no default; that is the whole difference. The shape reproduces with no
function anywhere:

```scala
trait Inv1 { final def d(x: Int, n: Int = 0)(s: String): Unit = … }
mk().d(1)("c")          // throws
inv.d(1)("c")           // fine — a path receiver is not hoisted
mk().d(1, 0)("c")       // fine — no default omitted
mk().g(1)               // fine — one clause
mk().h(1)()             // fine — the default is in the *last* clause
```

The hoist now happens at the outermost application of the chain, and
`name$default$n` arguments are re-pointed at the local wherever in the chain
they sit.

## 2. `asInstanceOf` / `isInstanceOf` on a primitive qualifier

`emit_as_instance_of` reads its receiver as an `Object` — which is what `Any`
erases to — so an `int` on the stack was either a `VerifyError` or an
`intValue()` on something that was never boxed. `i.asInstanceOf[Any]` came out
as `iload_1; areturn` from a method returning `Object`.

nsc's erasure settles this before the cast exists: primitive to primitive is a
*numeric conversion* (`i.asInstanceOf[Long]` is `i2l`, and to the same type it
is nothing at all), primitive to reference is a *box*.

slick's `StatementInvoker.iteratorTo` is

```scala
results(maxRows).fold(r => new CloseableIterator.Single[R](r.asInstanceOf[R]), identity)
```

over an `Either[Int, PositionedResultIterator[R]]`, so `r` is an `Int` and `R`
erases to `Object`. Every `.result` reaches it.

## 3. An applied `asInstanceOf`

`gen_apply`'s `peel_fun` strips a `TypeApply` to reach the callee, which is
right for `f[T](x)` and wrong for a cast: `f.asInstanceOf[A => B](v)` yields a
*value*, and the arguments belong to that value's `apply`. It came out as
`aload f; aload v; invokevirtual java/lang/Object.asInstanceOf()`.
`p06_update_tx`'s first `transactionally` hit it, in slick's `BasicBackend`:
`f.asInstanceOf[Any => DBIOAction[?, Streaming[T], Nothing]](v)`.

Anything else applied to a cast goes through an `apply` selection the typer
inserts, so this shape means a function value.

## 4. A value class's `name$default$n$extension` was missing from the module

`emit_value_extension_forwarders` walks the template body, and a default getter
is a synthesized *symbol* with no `DefDef` there. The `$extension` static
landed on the class (`emit_default_getters` puts it there) and nothing landed
on the companion module. nsc emits both.

Only a **separately compiled** caller links against the module copy, which is
why running our own build never saw it: `p02_queries` got
`NoSuchMethodError: StringColumnExtensionMethods$.like$default$2$extension`
from a client real scalac compiled, for
`def like(e: Rep[String], esc: Char = ' ')`.

## 5. A tuple-typed pattern binder had no `checkcast`

`emit_pattern_cast` narrows an extracted value through `type_jvm_name`, which
answers `java/lang/Object` for a structural `Type::Tuple` — i.e. no cast at
all. A `(TermSymbol, Node)` really is a `scala/Tuple2` at run time, so a binder
of that type read out of an erased extractor went straight from
`SeqFactory$UnapplySeqWrapper$.apply$extension(SeqOps, I)Object` into
`getfield scala/Tuple2._2`.

slick's `MergeToComprehensions` has
`case StructNode(ConstArray(ch, _*)) => ch._2`, so the whole class failed
verification and took every `groupBy` and every join with it. Ten lines
reproduce it (`case Seq(ch, _*) => ch._2`); `case h :: _ => h._2` was already
right, which is why it had not shown up before. `checkcast_internal` has
spelled tuples correctly all along — only this pattern path went through
`type_jvm_name`.

## 6. A narrowed override got a second mixin forwarder instead of a bridge

`emit_mixin_forwarders` keys on the name *and erased parameter list*, so a
concrete trait method and an override of it further down the linearization look
like two separate members whenever the two erase differently. Both got a
forwarder to their own trait's body, and the wide one — which is what a call
through the base interface resolves to — ran the base.

slick's `SynchronousDatabaseAction` declares

```scala
def openStream(context: C): CloseableIterator[Any] =
  throw new SlickException("Internal error: Streaming is not supported by this Action")
```

with `C <: BasicBackend#BasicActionContext`, and `StreamingInvokerAction`
overrides it at `C = JdbcBackend#JdbcActionContext`. `p12_mapped`'s `.result`
reached the throw.

nsc emits the wide descriptor as a bridge. Deciding which of two descriptors is
which needs two tests, and `narrower_override` does both: `bridge_overrides`
(already used by `emit_erasure_bridges`) says the two really are one member
rather than an overload, and `desc_narrows` fixes the *direction* —
`bridge_overrides` holds just as well with the arguments swapped, so on its own
it makes both methods bridge to each other.
`emit_inherited_covariant_bridges` cannot take this over: it declines whenever
the class already has the wide descriptor, and it only bridges reference
results, while `size(c: C): Int` in the same shape needs the same treatment.

The regression fixture is `tests/fixtures/selfrec.scala` (one file, all six)
with `crates/cli/tests/selfrec.rs`: real scalac 2.13.16's own stdout, a
receiver-evaluation counter for the hoist, and `javap` for the three things
stdout cannot see — the cast shapes, the module-side default getter, and the
bridge.

## Where it stops now

**Five of the six remaining failures are one defect, and it is the same one
`p09_plainsql` has been showing all along.** Inside a value class, a call to
another of its own methods does not reach the underlying value:

```scala
final class Ops(val s: String) extends AnyVal {
  def b(n: Int): String = s * n
  def a(n: Int): String = b(n) + "!"
}
```

```text
public java.lang.String a(int);           // the instance method
   7: aload_0                             // ← `this`, an `Ops`
   8: iload_1
   9: invokestatic  b$extension:(Ljava/lang/String;I)Ljava/lang/String;

public static java.lang.String a$extension(java.lang.String, int);
   7: new  #8  // class Ops               // ← re-boxes slot 0
  11: aload_0
  12: invokespecial "<init>":(Ljava/lang/String;)V
  15: iload_1
  16: invokestatic  b$extension:(Ljava/lang/String;I)Ljava/lang/String;
```

Both want the underlying value: `aload_0; invokevirtual s()` in the instance
method, and plain `aload_0` in the `$extension` static (slot 0 already holds
it). The first shape is `p09_plainsql`'s
`ActionBasedSQLInterpolation.sqlu`; the second is what
`AnyOptionExtensionMethods.map$extension` does to
`flatMap$extension`, handing `OptionLift.baseValue` a wrapper instead of a
`Rep` — `scala.MatchError: slick.lifted.AnyOptionExtensionMethods` in
`p03_joins`, `p05_options`, `p07_caseclass` and `p11_sqlgen`. Interfaces are
not checked by the verifier, which is why the second shape links and fails at
run time. Six lines reproduce both, with no classpath.

`p10_types` is separate: `NoSuchMethodError:
RelationalProfile$ColumnOption$Length$.apply$default$2()`, a case class's
default-argument getter on a *nested* companion module. Defect 4 above is a
different one (a value class's, and only the module copy was missing).

The two that run to completion differ in ways that are much later questions
than not running at all:

* **`p02_queries`** prints `{fn length("NAME")}` where nsc's build prints
  `length("NAME")` — an `H2Profile` override of the JDBC-escape spelling is not
  taking effect.
* **`p06_update_tx`** does not roll back: `afterTx2` keeps the update the
  transaction threw out of, and `seq=List(2, 2)` instead of `List(2, 1)`.

# Ten of the twelve (`agent/vcself`)

```text
progs=12 ok=10 diff=1 fail=1
```

`p01_basic`, `p02_queries`, `p03_joins`, `p04_groupby`, `p05_options`,
`p07_caseclass`, `p08_mapto`, `p09_plainsql`, `p11_sqlgen` and `p12_mapped`
print, byte for byte, what the scalac-built slick prints. Three defects, all
of them already on `main`.

## 1. A value class calling its own methods never reached the underlying value

The assignment, and the previous note's reading of it is right — measured
against real scalac 2.13.16 on six lines with no classpath:

```scala
final class Ops(val s: String) extends AnyVal {
  def b(n: Int): String = s * n
  def a(n: Int): String = b(n) + "!"
}
```

Every method of `class C(val u: U) extends AnyVal` is really a static taking
`u`, and `this` inside `C` is the box. The two meet at exactly one place — the
receiver of a call from `C` to another of `C`'s own methods — and neither half
was doing the conversion:

* the instance method `a(int)` pushed `aload_0` (an `Ops`) into
  `b$extension(String, int)`; nsc emits `aload_0; invokevirtual s()`;
* the static `a$extension(String, int)` re-boxed slot 0 — which *is* the
  underlying value — with a `new Ops(u)` before handing it on; nsc emits the
  bare slot.

`gen_value_self_receiver` is that one place. `load_this`'s existing
`new C(u)`-on-demand stays: a lambda lifted out of an `$extension` really does
want the box for its `$outer`, and unwraps it again through the accessor,
which is why the helper keys on `ctx.value_ext` only when `ctx.outer` /
`ctx.outer_slot` say the body is not inside a lifted lambda.

A *no-argument* member is not affected: `def q = p + p` goes out as
`aload_0; invokevirtual p()` on the instance method that scala-rs emits beside
each `$extension`, which is correct (nsc routes it through the module instead).
`q$extension` does build a box for it, and that is what nsc's own instance
method amounts to.

This was `p09_plainsql`'s `ActionBasedSQLInterpolation.sqlu` (the instance
shape, a `VerifyError` because `StringContext` is a class) and
`AnyOptionExtensionMethods.map$extension` → `flatMap$extension` (the static
shape — `OptionLift.baseValue` got a wrapper and threw `MatchError`) in
`p03_joins`, `p05_options`, `p07_caseclass` and `p11_sqlgen`. Only two of
those four became `ok` on this fix alone; the other two were behind defect 3.

## 2. An erasure bridge over a `Unit` result returned nothing

```scala
trait SP[-T] extends ((T, String) => Unit)
object SetUnit extends SP[Unit] { def apply(none: Unit, pp: String): Unit = () }
```

`SetUnit$.apply(Object, Object)Object` came out
`invokevirtual apply(BoxedUnit, String)V; areturn` —
`VerifyError: Operand stack underflow`. `Unit` is `V` as a method *result* and
`Lscala/runtime/BoxedUnit;` everywhere else, and `param_adapt`'s `Unit` rule
("a `Unit` argument already is a `BoxedUnit` reference; adapt nothing") is the
parameter rule. In return position the call leaves the stack empty and the
bridge owes a reference: nsc pushes `BoxedUnit.UNIT`, and
`emit_erasure_bridges` now does. `emit_inherited_covariant_bridges` never
picked such a target (it requires a reference result at both ends).

slick's `implicit object SetUnit extends SetParameter[Unit]` is this shape, so
it was every plain-SQL statement — the blocker `p09_plainsql` reached once
defect 1 was gone.

## 3. A `val` narrowed by a subclass got no wide getter

`emit_inherited_covariant_bridges` accepted a `SymKind::Term` parent member
only from a *trait*, on the reasoning that a trait `val` is an interface method
too. A class `val` is a getter just as much:

```scala
class Base { protected val q: Option[Seq[Int]] = None
             def show = if (q.forall(_.contains(1))) "quote" else "bare" }
class Sub extends Base { override protected val q: Some[Nil.type] = Some(Nil) }
```

`Sub` declared only `q()Lscala/Some;`, so `Base.show`'s
`invokevirtual Base.q:()Lscala/Option;` read `Base`'s own field and answered
`quote` for a `Sub`. The guard is now "not `private`" instead of "the parent is
an interface".

This is the `{fn …}` difference the previous note left open, and it was worth
three programs rather than one. slick's
`JdbcStatementBuilderComponent.QueryBuilder` has

```scala
protected val quotedJdbcFns: Option[Seq[Library.JdbcFunction]] = None // quote all by default
```

and `H2Profile`'s `QueryBuilder` subclass overrides it with `Some(Nil)`;
`val quote = quotedJdbcFns.forall(_.contains(sym))` therefore stayed `true`.
`p02_queries`, `p07_caseclass` and `p11_sqlgen` all print generated SQL
containing a JDBC function.

The regression fixture is `tests/fixtures/vcself.scala` (one file, all three)
with `crates/cli/tests/vcself.rs`: real scalac 2.13.16's own stdout, plus
`javap` for the three things stdout cannot see — the two receiver shapes, the
`BoxedUnit.UNIT` in the bridge, and the wide getter's descriptor, which only a
separately compiled caller links against.

## Where it stops now

* **`p10_types`** — unchanged, and not this slice's:
  `NoSuchMethodError: RelationalProfile$ColumnOption$Length$.apply$default$2()`
  on `O.Length(64)`, a case class's default-argument getter on a *nested*
  companion module.
* **`p06_update_tx`** — unchanged: the transaction does not roll back,
  `afterTx2` keeps the update it threw out of and `seq=List(2, 2)` instead of
  `List(2, 1)`.

# All twelve (`agent/lasttwo`)

```text
progs=12 ok=12 diff=0 fail=0
```

Every one of the twelve slick client programs — compiled once by real scalac
2.13.16 against the scalac-built slick, then run against each of the two slick
builds — prints, byte for byte, what the scalac build prints. The classfiles
under them are scala-rs's, and the run reaches H2 through the query compiler,
the SQL generator, JDBC and result mapping.

The assignment was two programs and it turned out to be **four defects**, plus
a fifth found on the way that slick does not exercise. `p10_types` alone was
three of them, one behind the other: the brief's guess that its
`apply$default$2` might share a root with `agent/missingclasses`'s
"constructor default arguments" note was right, and fixing it uncovered two
more, both `AbstractMethodError`/`ClassCastException` in the same class.

All five were on `main`; none is a regression from any slice.

## 1. A primary constructor's defaults had no getters at all

`p10_types`: `NoSuchMethodError:
RelationalProfile$ColumnOption$Length$.apply$default$2()`.

nsc puts a constructor's defaults on the class's **companion module**, under
two names — `$lessinit$greater$default$n` for `new C(…)` and, for a case
class, `apply$default$n` for the synthetic `apply`. scala-rs synthesized
neither: `Typer::type_default_rhs_here` splices the stored expression into the
call instead, which is enough inside one run and invisible from outside it. A
*separately compiled* caller emits the getter call, and slick's

```scala
case class Length(length: Int, varying: Boolean = true) extends ColumnOption[Nothing]
```

is reached from client code as `O.Length(64)`.

`crates/typer/src/ctor_defaults.rs` declares both getters on the companion
module class with the default's typed body; `Gen::emit_default_getters`
already writes out every `$default$` member of a class it emits, so
`emit_case_companion` only had to start calling it. This is the same root
`docs/not-implemented.md` recorded as "default constructor arguments across a
compilation run", and it is now closed for every class that *has* a companion
— which is every case class, and any class the source gives an `object`. The
part that is left is nsc *synthesizing* a companion for a plain
`class Box(val a: Int, val b: Int = 7)`; that would add classfiles.

Two details that are not obvious and that slick needs:

* **The result type is inferred, not declared, when the parameter's type names
  one of the class's type parameters.** `case class Comprehension[+Fetch <:
  Option[Node]](…, fetch: Fetch = None, …)` has no `None` that conforms to
  `Fetch`; nsc's getter is declared `scala.None$`. Declaring `Fetch` made
  slick fail to compile at the declaration.
* **The typed body goes on the getter only.** The parameter's own
  `default_rhs` has to stay the namer's untyped tree, because that is what a
  call site clones and re-types in its own scope.

The getters are *not* consulted at call sites: a case class's synthetic
`apply` keeps splicing. Calling them would fix the argument's type before the
class's type parameters are solved, and `Comprehension(s, n, select = …)` then
reports `found: None$ required: Fetch`. nsc infers `Fetch` **from** the
getter's result; scala-rs does not, and that gap is now written down in
`docs/not-implemented.md` rather than guessed at.

## 2. A default getter reached through an *inserted* `apply`

Latent until defect 1 put such getters on companions, and a real bug on its
own: `G.H(4)` is `Select(G, "H")` carrying `H`'s `apply` as its symbol, so
`default_getter_apply`'s "the receiver is the qualifier" answered `G` and the
call came out as `G$.apply$default$2` — a compile error against a program real
scalac accepts. The head of the application chain is the receiver whenever the
`apply` was inserted.

## 3. A trait nested in a member `object`, mixed in elsewhere

Two halves, both in `p10_types`'s `items.schema`.

slick has

```scala
trait JdbcStatementBuilderComponent { self: JdbcProfile =>
  class TableDDLBuilder(table: Table[?]) { … }
  object TableDDLBuilder {
    trait UniqueIndexAsConstraint extends TableDDLBuilder { … }
  }
}
```

and `H2Profile` mixes it in with
`class H2TableDDLBuilder(table) extends TableDDLBuilder(table) with
TableDDLBuilder.UniqueIndexAsConstraint`.

* `emit_trait_outer_accessors` declined to implement
  `…$UniqueIndexAsConstraint$$$outer()` at all, because `TableDDLBuilder$` is
  not on `H2TableDDLBuilder`'s own `$outer` chain — that chain runs to
  `H2Profile`. A member `object` is reached through the enclosing template's
  accessor (`H2Profile.TableDDLBuilder()`), not by walking out.
  `AbstractMethodError` on the first `createIndex`.
* Inside the trait's body, `table` and `columns` — members of the class the
  trait **extends** — were read by walking out to `$outer` and casting that to
  `TableDDLBuilder`. `is_owner_compatible` refuses to follow a trait's class
  parent, which is right for a call *through the interface* and wrong for
  `this`: every instance of `trait U extends B` is a `B`, and nsc emits
  `aload_0; checkcast B`. `ClassCastException: H2Profile$ cannot be cast to
  …$TableDDLBuilder`. `load_owner_instance` now uses `self_reaches_owner`,
  which does follow that edge; the trailing `checkcast` was already there.

## 4. A mixin forwarder overrode a superclass's own override

This is `p06_update_tx`, and it was not what either the brief or the earlier
note guessed. The transaction never *started*: a probe
(`SimpleDBIO(_.connection.getAutoCommit)` run inside `.transactionally`
against each build) printed `false` on the scalac build and `true` on ours, so
there was nothing to roll back. `transactionDepth` was right and
`installSession` did take its `setupTransaction` branch — the branch just ran
`BasicBackend`'s `= None` instead of `JdbcBackend`'s override.

`emit_mixin_forwarders` keys on name plus *erased parameter list*.
`BasicDatabaseDef.setupTransaction(session: Session, …)` erases to
`(BasicSessionDef, Option)`; `abstract class JdbcDatabaseDef` fixes
`type Session = JdbcSessionDef` and overrides it, which erases to
`(JdbcSessionDef, Option)` plus a bridge for the wide one. Different key, so
`new JdbcDatabaseDef(…){}` — the anonymous class every `Database` really is —
was handed a forwarder for the wide descriptor straight to
`BasicDatabaseDef$class.setupTransaction`, and that forwarder overrode the
bridge it inherited.

The rule now follows the linearization: a trait that sits **past the
superclass** in it is an ancestor of that class, not a mixin of this one, and
if a class on the superclass chain declares a concrete member that
`bridge_overrides` it, this class owes no forwarder. Traits mixed in by the
class itself come *before* the superclass and are untouched, so
`class B extends A with T` where `T` overrides `A.m` still forwards to `T`.

## 5. A `case class` in a trait had no companion accessor

Found while probing 4, and not on slick's own path. A `case class` declared in
a trait carries a *synthesized* companion, which is a member `object` of that
trait exactly as a written one is: the trait declares an abstract `K()`
accessor and every class mixing it in owes an implementation. Only the written
`ModuleDef`s were harvested into `TraitImpls::modules`, so nothing implemented
it:

```scala
trait T { case class K(a: Int, b: Int = 2) }
object P extends T
P.K(1)   // AbstractMethodError: P$ … abstract T$K$ K()
```

slick's `trait BasicBackend { case class ExecState(…) }` is this shape and
survives only because nothing outside the trait names `ExecState`.

## Corrections to the record

* The brief's "`p10_types` may or may not share a root with `agent/
  missingclasses`'s constructor-defaults note" — it does, and that note's
  reading ("namer has to synthesize `$lessinit$greater$default$N` in the
  companion body") is right in substance. It does not need a `DefDef` in the
  body, though: a symbol on the companion module class with the typed body on
  it is enough, because `emit_default_getters` already walks those.
* The brief's "`p06_update_tx` does not roll back — the `.transactionally`
  path" named the right program and the wrong mechanism. Nothing about
  `Outcome.isSuccess`, `guaranteeCase` or the commit/rollback choice was
  wrong; the connection was in autocommit the whole time.
* "One failing program, one root" held for neither. `p10_types` was three.

## Verification

On `agent/lasttwo` **after `git merge main`**:

| | |
|---|---|
| `tests/slick_run.sh` | `progs=12 ok=12 diff=0 fail=0` |
| `tests/slick_measure.sh` | `files=184 errors=0 files_with_errors=0 classes=1596` |
| `tests/slick_subset.sh` | `verified=1596 failed=0` |
| `cargo test --workspace --release` | 1909 tests, 0 failed (142 binaries) |
| `tests/conform/` | 86 passed |
| `javap -p` over all 1596 slick classfiles | clean |

The regression fixture is `tests/fixtures/lasttwo.scala` (one file, all five)
with `crates/cli/tests/lasttwo.rs`: real scalac 2.13.16's own stdout, plus
`javap` for the four things stdout cannot see — the companion's default-getter
descriptors (only a separately compiled caller links against them, and
`scala.None$` is the inferred one), the module accessor behind the trait's
`$outer`, the absence of a forwarder on the anonymous subclass, and the
case-class companion accessor on a trait and its implementor.

## `p03_joins` is flaky under load (2026-09-05)

`agent/lazysig2` saw `RUN-FAIL p03_joins rs=1 scalac=0` twice while the
machine was also running `cargo build`, and 12/12 on the next run of the same
script with the same binary. What the failure looks like:

* `a.out` and `b.out` are **byte-identical** and complete — every line the
  program prints, including the last.
* `a.err` holds nothing but SLF4J's "no providers" warning. No exception.
* Only the exit status differs, and only for the scala-rs-built run.

It is not a compiler difference. Compiling slick five times with one binary
gives a byte-identical class tree (`find -name '*.class' | sort | xargs cat |
md5`), and running the client classfiles 12 times against the scala-rs build
and 12 against the scalac build gives 24 clean exits. `p03_joins` is the only
program that uses `cats.effect.unsafe.implicits.global`, whose runtime installs
a shutdown hook; a shutdown that does not finish in time is the obvious
suspect, and it only misses under load.

**Before treating a `RUN-FAIL` here as a regression, check whether `a.out`
and `b.out` are identical.** If they are, the compiler produced a working
program and the harness caught a shutdown, not a bug.
