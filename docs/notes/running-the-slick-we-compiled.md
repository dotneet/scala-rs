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

`tests/slick_run.sh` is still **0 of 12**: every program, `p12_mapped`
included, now stops in `Database.make` on the `fs2.Stream.fromIterator` value
class from `-cp` — `agent/cpvalueclass`'s slice, and the same
`VerifyError: Type integer is not assignable to 'java/lang/Object'` for all
twelve. `p12`'s own defect is gone: it no longer fails differently.

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
