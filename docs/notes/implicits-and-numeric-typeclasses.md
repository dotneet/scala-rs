# Implicit resolution and the numeric / collection type classes

Four slices from the scala-rs development log about implicit search and the
type-class hierarchies the 2.13 standard library is built on: the leftover
implicit bugs and prelude gaps found while chasing slick, the extension methods
`import cats.syntax.all._` brings in, the `BuildFrom` machinery that decides what
a collection transform actually returns, and giving `Integral` / `Fractional`
their real place under `Numeric`.

### Implicit leftovers and prelude gaps (`agent/impltail`)

This slice chases the implicit-related errors that were still left in slick. The fixtures are
`tests/fixtures/itail.scala` (the accepting case; stdout is byte-for-byte identical to real scalac 2.13.16) and
`tests/fixtures/itail_bad.scala` (the rejecting case), and the test is `crates/cli/tests/impltail.rs`.

| `itail.scala` (`crates/cli/tests/impltail.rs`, library dual-run) | Re-typing a call whose implicits were already filled in (the tupling retry), `Numeric[T] <: Ordering[T]`, a type parameter that only implicit search can decide, `apply` on a function value, a residual implicit clause in argument position (`take(Array.empty)`), a case class with a repeated parameter | `Pair(Lit(1, …))` `Int 42 true` `-1` `a:str0` `b:bool1` `n=2 n=0` `0` `r 6 3` `true` `0` |

The minimal accepting/rejecting tests live in the same file
(`an_implicit_filled_call_survives_being_typed_twice` /
`numeric_is_an_ordering` / `a_numeric_type_parameter_is_an_ordering` /
`an_implicit_only_type_parameter_is_solved_by_the_witness` /
`apply_on_a_function_value_is_the_function` /
`a_residual_implicit_clause_is_applied_in_argument_position` /
`the_parameter_decides_which_witness_a_residual_clause_needs` /
`an_implicit_object_is_not_ambiguous_with_itself` /
`a_repeated_case_class_parameter_has_a_sequence_default`).
`itail_bad.scala` locks in that both a residual implicit clause with no witness and an
implicit-only type parameter with no candidates at all get diagnostics that say the same thing nsc's do.

The measurement is `files=184 errors=833 files_with_errors=102` → `errors=777 files_with_errors=93`.


### Extension methods from cats syntax (`agent/catsyntax`)

The `fa.flatMap(…)` / `a >> b` / `fa.attempt` that `import cats.syntax.all._` brings into scope
**now resolve against the real cats (cats-core 2.13.0 / cats-effect 3.7.1)**.
There are five gaps, including the refinement one whose root cause `agent/catsimpl` had already identified.
The test is `crates/cli/tests/catsyntax.rs`, and the fixture prefix is `csyn`.

1. **The first type argument of a higher-kinded class is not the "element"** (`agent/catsimpl` had
   reported this as a separate bug). `map` / `flatMap` / `foreach` / `withFilter` / `pipe` /
   `tap` were replacing the lambda's parameter type with the receiver's **first type argument**.
   That is right for `List[A]` and wrong for cats' `Ops[F[_], A]`.
   The `n` in `new Ops[Box, Int](b).flatMap(n => …)` came out as `Box`, and
   `n + 1` fell through to `any2stringadd` (`csyn_ops`; reproduced without any implicit conversion).
   We only use the first type argument as the element when its kind arity is 0. On the result-type side,
   the "if it mentions an undetermined `B`, widen to `Any`" handling stays exactly as it was
   (removing both made `fa.flatMap(_ => fa)` come out as `F[Any]`).

2. **Convert pickle's `REFINEDtpe` into `Type::Refined`**. The result type of every
   `toFooOps` that simulacrum generates is `Foo.Ops[F, A] { type TypeClassType = Foo[F] }`, and
   `PickleSupply::conv` could not represent that shape, so the members were not supplied at all
   (`unmappable result type Refined { … }`). I added `conv_refined` / `conv_refine_decl`.
   The **never drop things silently** policy is unchanged. If even one parent or one declaration
   fails to convert, the whole refinement is declined, and `SCALA_RS_PICKLE_DEBUG=1` prints
   the reason (which parent / which declaration). Shapes that do not fit into `RefineDecl`, such as a `def`
   with type parameters, return `None`. Only a refinement with one parent and no declarations
   becomes that parent itself, since nothing is lost that way.
   The receiving side needed two spots as well: a `Type::Refined` arm in `subst_as_seen_from`'s `walk`
   (without following the parent, the `A` of `Ops[F, Int]#flatMap` stays bare), and
   `elem_type` looking through a refinement's parent.
   I also made **a nested class such as `cats.FlatMap.Ops` search only the owner's direct declarations**
   (`find_or_stub_java_class`). Because it was following parents too,
   `cats/FlatMap$Ops` asked `FlatMap` for `Ops` and got back `Functor.Ops`, which sat
   further along the linearization.

3. **`import o._` brings in the members `o` *has*** (SLS 4.7). `cats.syntax.all`
   declares almost nothing itself: both `toFlatMapOps` and `catsSyntaxApplicativeId` live on the
   roughly 60 traits it mixes in. Since we were only bringing in direct members,
   not a single one of cats' syntax layers was in scope.
   We now walk the parents breadth-first and bring those in, but **when the same extension arrives by two
   paths, we keep one** (`Typer::drop_inherited_duplicates`). The prelude puts some of the library's conversions
   directly on the inheriting objects as well, so as things stood `xs.asJava`
   was left undecided by "two conversions that return the same member with the same result type", and
   `scala.jdk.CollectionConverters._` broke.
   The codegen side needs the receiver too (`Typer::wildcard_module_for`). Emitting an inherited conversion
   under its bare name pushes `this` and checkcasts to the trait that declared it, which gives
   `Main$ cannot be cast to tinycats.FlatMap$ToFlatMapOps`.
   The receiver is the imported object.

4. **`InnerClasses` is not a table of "the nested classes this class declared"**.
   `cats/effect/kernel/MonadCancel.class` lists `cats/syntax/package$all$`
   (because it references it). We were taking that at face value, so `cats.syntax.all` came in
   as a member of `MonadCancel`, and since `load_binary_into` only ever completes a classfile once,
   an `import cats.syntax.all._` arriving later became
   `value all is not a member of <notype>`.
   It only happens **when you write `import cats.effect.… ` first**, so it looked like a quirk of
   import order, but slick's `BasicBackend.scala` uses exactly that order.
   We now only take entries whose prefix is the class's own JVM name.

5. **Solve the conversion's type arguments from its own implicit clause** (`solve_conv_targs_from_implicits`).
   The `E` in `catsSyntaxApplicativeError[F[_], E, A](fa: F[A])(implicit F: ApplicativeError[F, E])`
   appears nowhere in `F[A]`, so it fell back to `AnyRef` and `fa.attempt` ended up with the type
   `F[Either[AnyRef, A]]`, which "resolves but conforms to nothing".
   A witness in scope (`Async[F] <: MonadError[F, Throwable]`) is what decides
   `E = Throwable`. We only search for type arguments that the result type mentions
   (because it runs an implicit search per candidate).
   Along with that, we warm up the receiver's implicit scope at the top of `search_extension`.
   Whether a higher-kinded conversion is applicable is decided by "is there a witness for its own implicit clause"
   (`agent/catsimpl`), and that search takes `&self`, so it cannot load anything itself.
   The witness for `FlatMap[Box]` is `Box`'s companion, a separate classfile that nobody asks for.

The measurement is `files=184 errors=537 files_with_errors=80` → **`errors=529
files_with_errors=80`**. The raw count barely moves, but **the kind of error this slice was
aiming at is gone**. `… is not a member of F[…]` (`flatMap` / `>>` /
`attempt` / `map` / `void` / `timeoutTo` / `guarantee` …) went **42 → 8**, and
`value all is not a member of <notype>` (item 4) went **2 → 0**.
The remaining 8 (4 of `value flatMap is not a member of F` and
4 of `value >> is not a member of F`) **all have a bare `F` as the receiver**,
and that was fixed in the next slice, `agent/companionkind`.
The net difference is only 8 because, now that the extension methods resolve,
**the cascade that used to be stopped behind them is out in the open** (`found: F required: F[Unit]`,
`no matching overload for (Function0[A])F` and so on; all of them caused by the same bare `F`).


### Result types of collection transform methods (`BuildFrom`, `agent/buildfrom`)

In 2.13, collections decide the result type of `map` and friends through
`BuildFrom` / `IterableFactory` / `MapFactory` and `CC[_]`. None of that was
working in scala-rs, so results fell back to a supertype collection.

```scala
val m: Map[String, List[Int]] = Map("x" -> List(1,2))
m.map { case (d, g) => d -> g.sum }   // scalac: Map(x -> 3)
// scala-rs: found: Iterable[Tuple2[String, Int]] required: Map[String, Int]
```

I built a table of the major collections against the major methods (`List` / `Vector` / `Seq` /
`IndexedSeq` / `Set` / `Map` / `SortedMap` / `TreeMap` / `TreeSet` /
`ArrayBuffer` / `ListBuffer` / `LazyList` / `Array` / `String` against `map` /
`flatMap` / `collect` / `filter` / `filterNot` / `++` / `zip` / `groupBy` /
`groupMap` / `groupMapReduce` / `partition` / `to` / `sorted` / `reverse` /
`distinct` / `take` / `drop` / `updated` / `-` / `+` — 308 combinations), and diffing it against
real scalac 2.13.16 showed **99 disagreements**. There were five causes.

1. **A curried call solved every clause's types against the declaration's "first" clause.**
   `Typer::instantiate_from_call` unconditionally read `paramss.first()` of
   `self.st.get(sym).ty`, so for
   `def f[K, B](k: A => K)(g: A => B)(r: (B, B) => B)` it solved `K` twice and
   never solved `B` at all. That is why the `reduce` of
   `groupMapReduce(key)(f)(reduce)` came out as `(Any, Any) => Any` and `_ + _`
   surfaced as `no matching overload for (String)String with arguments (Any)` —
   **an error that looked like it belonged to an unrelated line**. It now passes the
   number of consumed clauses (`s_paramss.len() - paramss_ids.len()`) and solves against
   the declared type of *that* clause.

2. **`BuildFrom` itself.** I added a single `Typer::rebuild_from_receiver` that rebuilds the
   declared result type `D[…]` in terms of the receiver's root class `R`. It only fires when
   `R` is a proper subclass of `D` and is a `scala.collection` class
   (`maps_to_own_class`). If `R` and `D` take the same number of type arguments it
   substitutes directly; if `R` takes two and `D` takes one and is being handed a pair, it
   unpacks the pair — which is exactly the difference between
   `public default <K2, V2> CC map(Function1<Tuple2<K, V>, Tuple2<K2, V2>>)` in
   `javap -p -s scala.collection.MapOps` and `<B> CC map(Function1<A, B>)` in
   `IterableOps`. A lambda that does not return a pair stays `Iterable[B]`, which is what
   nsc infers too. `partition` returns `(C, C)` and `groupBy` / `groupMap` return
   `Map[K, C]`, so the **inside** of the result gets rebuilt as well (`rebuild_inside`).
   For a curried `groupMap(k)(f)` the receiver sits behind a `Select`, so it is reached via
   `curried_receiver_ty`.

3. **Removing the `erases_to_object` gate.** The narrowing for `filter` / `take` / `++` and
   the rest was restricted to "only when the descriptor returns `Object`".
   `TreeMap - key` returns
   `(Object)Lscala/collection/immutable/Map;` and so was excluded, and the README even said
   "the result type of the Apply has to survive erasure".
   `maybe_unbox_erased_result` had since been taught to **emit a checkcast for a result type
   narrower than the declaration**, so the gate was stale. With it removed, the places where
   the stdlib dispatch that writes descriptors by hand
   (`+` / `-` / `++` / `filter` / `map` / `updated` under `is_stdlib_map` / `is_stdlib_set`)
   emitted a fixed `checkcast` were replaced by `cast_collection_result`, which casts to the
   type the typer decided when that is a subclass of the declaring class. Without this,
   `s.copy(waiting = s.waiting - key)` becomes
   `VerifyError: Bad type on operand stack`.
   I added `-` / `+` / `--` / `removed` / `incl` / `excl` / `concat` to
   `returns_receiver_collection` (`1 + 2` and `"a" + b` go through this path too, but
   nothing is rebuilt unless the receiver is a `scala.collection` class, so they are
   untouched).

4. **Selecting the `Map.map` overload in codegen as well.** `MapOps.map` *builds a map*, so
   its function has to return a pair, and 2.13 picks `IterableOps.map` when it does not.
   scala-rs only has one symbol, the pair-taking one, so when the result type is not a pair
   (or a map) it now calls
   `IterableOps.map:(Lscala/Function1;)Ljava/lang/Object;`.
   Without this, `m.map { case (_, v) => v }` dies with
   `ClassCastException: Integer cannot be cast to Tuple2`.

5. **The `Factory` for `xs.to(ArrayBuffer)`** (a leftover from `agent/ambigmap`).
   The argument of `to[C1](factory: Factory[A, C1]): C1` is a companion *object*, not a
   `Factory`. The bridge is a **view**,
   `object IterableFactory { implicit def toFactory[A, CC[_]](factory:
   IterableFactory[CC]): Factory[A, CC[A]] }`, and three things were needed.
   - A parent edge `IterableFactory[CC]` / `MapFactory[CC]` on the companion
     (`prelude_buildfrom.rs`). This is an **edge for conformance only**, so members from the
     factory trait are dropped — the classfile-derived `apply` / `empty` return the trait's
     own abstract `CC`, so inheriting them would turn
     `mutable.ArrayBuffer[Int]()` into `ArrayBuffer[A]`.
   - Re-declaring `toFactory` in the prelude. A Java generic signature cannot write
     `CC[A]`, so the classfile side reads `<A, CC> Factory<A, CC>` and solves
     `C1 = ArrayBuffer` (the bare type constructor). The fact that it is `implicit` also
     lives only in the pickle, and `PickleSupply::supply_implicit_members` deliberately
     skips `scala/`.
   - **View search with undetermined type parameters still in the expected type**
     (`Typer::search_conversion_open` / `apply_open_views`). nsc's
     `inferView` runs with `Context.undetparams` in hand. The old `conversion_provides`,
     which compares declared types against each other, never bound `C1`, so no conversion
     was applicable. The conversion's own type arguments are now solved from the argument
     first (reading the argument **through the parameter's class** —
     `align_to_param_class`), and whatever is left plus the caller's undetermined variables
     are solved with a two-sided `Unify`. The `implicitly[Factory[Int, Vector[Int]]]` side
     is a **value**, not a view, so it is closed by declaring the companion's
     `implicit def iterableFactory[A]: Factory[A, CC[A]]` (javap:
     `List$.<A> Factory<A, List<A>> iterableFactory()`).

Three small gaps fell out along the way. The lower bound of `[B >: A]` was being
**substituted with the receiver's "bare type arguments"** (`Map[K, V]` inherits
`++` from `IterableOps[A, …]` where `A = (K, V)`, but it became `A := K`, so
`Map("a" -> 1) ++ Map("b" -> 2)` came out as `Iterable[Serializable]` — and since
it only happened in a file that had already completed `IterableOps.++` against a different
receiver, it was another of those that **look like a bug on an unrelated line**), so it now
reads through the base type at the owner (`owner_args_as_seen_from`; `check_tparam_bounds`
does the same). `immutable.Set.++` is `SetOps.concat(IterableOnce[A])` but was declared as
taking `Set[A]`. And `mutable.Map` had no `-` at all
(javap: `mutable.MapOps.$minus(K)`,
`(Ljava/lang/Object;)Lscala/collection/mutable/MapOps;`).

The table's disagreements went from **99 to 12**. slick went
`errors 354 → 339`, with `files_with_errors` unchanged at 65.

**Understood but not fixed:**

- **`map` / `flatMap` / `collect` on sorted maps.** `TreeMap.map` is
  `SortedMapOps.map[K2, V2](f)(implicit ord: Ordering[K2]): CC[K2, V2]`
  (javap: `(Lscala/Function1;Lscala/math/Ordering;)Lscala/collection/Map;`),
  and unless a witness is passed it falls back to `MapOps.map` and produces a plain `Map`.
  Narrowing only the static type to `TreeMap` would give a `ClassCastException` on
  assignment, so `rebuild_widened` does not rebuild for a sorted receiver. `filter` /
  `take` / `-` / `+` / `updated` need no witness and are unchanged.
- **`TreeSet.map` / `flatMap` / `collect` / `zip`** are `ambiguous overload`.
  Both `IterableOps.map[B]` and
  `SortedSetOps.map[B](f)(implicit ord: Ordering[B])` are applicable, and nsc picks
  "the one whose declaring class is the subclass". Overload specificity does not take the
  owners' subclass relationship into account.
- **`SortedMap.keySet` / `TreeMap.keySet`** return `Set` rather than `SortedSet` /
  `TreeSet` (`SortedMapOps.keySet` needs separate handling).
- **`Array.to(…)` / `Array.groupMapReduce`** are not members of `ArrayOps`; in 2.13 they go
  through `ArraySeq`. Likewise `"abc".zip(…)` goes through `WrappedString`, so it comes out
  as `Iterable` rather than `IndexedSeq`.
- **The `K$` of `Map.groupBy(f)`** becomes `Any` when the lambda returns something other
  than the key type (`m.groupBy(_._2 > 1)` gives `Map[Any, Map[Any, Int]]`).
  This is an inference gap that predates the slice, and I did not touch it here.
- Two new errors appeared in slick,
  `found: TypedType[Option[Option[Any]]] required: TypedType[Option[Any]]`
  (`lifted/OptionMapper.scala`). I have not managed to minimise them and the cause is
  unidentified.


### Remaining

- **Using something that returns `Nothing` in value position gives a `VerifyError`**
  (confirmed in `agent/lazyref`; the same on main. An existing bug unrelated to local
  `lazy val`s). Using `def boom: Nothing = throw new RuntimeException("x")` as in
  `if (n > 0) 1 else boom` erases the `Nothing` result to `V`, so only one branch leaves the
  stack empty and you get `Inconsistent stackmap frames`.
  `lazy val boom: Nothing = throw …` ends up on the same path (once lifted into an
  accessor), so it fails the same way. Fixing it requires marking the point after a
  `Nothing`-returning call as unreachable and pushing a dummy of the expected type.

- ~~**`lazyZip` on `Seq` / `IndexedSeq`** (confirmed in `agent/ambigmap`)~~.
  `lazyZip` itself later arrived through the pickle, and the `BuildFrom` for
  `LazyZip2.map` became solvable through the
  "Higher-order implicit matching for `BuildFrom` (LazyZip)" work
  (`agent/buildfrom2`).

- **`xs.to(ArrayBuffer)`** (confirmed in `agent/ambigmap`).
  The `Factory[A, C]` implicit cannot be obtained from the companion, so it comes out as
  `no matching overload for (Factory[Any, C1])C1 with arguments (ArrayBuffer$)`
  (`memory/HeapBackend.scala`). This is another place that only became visible once `map`
  worked; immediately before, the same line was stopping at
  `ambiguous overload for map`.

### Putting `Integral` / `Fractional` into the type-class hierarchy (`agent/integral`)

One item the previous section left open.

```scala
println(List.range(0, 5))   // error: no implicit: could not find implicit value of type Integral[Int]
println(Vector.range(0, 3)) // same
println(Seq.range(0, 3))    // same
```

`IterableFactory#range[A](start: A, end: A)(implicit ord: Integral[A])` is the real
signature (`javap -p scala.collection.IterableFactory`), and underneath it there were
**two** gaps.

1. `Integral` / `Fractional` are not in the symbol table at prelude time.
   When the source mentions the name, `pickle_supply` wakes a stub, but attaching the
   pickled parent (`Numeric`) is done by `attach_parents`, i.e. **only when member
   resolution fails**. With `SCALA_RS_PICKLE_DEBUG=1` you see
   `#quot: asking Integral` followed by `attaching pickled parent Numeric` — nowhere near
   in time for the subtyping decision. That is why
   `def f(x: Integral[Int]): Numeric[Int] = x` was a `type mismatch`.
2. The implicit instances in `object Numeric` were being given the type `Numeric[Int]`.
   The real ABI is one level further down.

The shape, confirmed with `javap -p -s /tmp/scala-rs-lib/scala-library-2.13.16.jar`:

```
interface scala.math.Numeric<T>    extends scala.math.Ordering<T>
interface scala.math.Integral<T>   extends scala.math.Numeric<T>
interface scala.math.Fractional<T> extends scala.math.Numeric<T>
```

| implicit object (`Numeric$…$`) | implements | that trait's parent | type we give it |
|---|---|---|---|
| `IntIsIntegral$` | `Numeric$IntIsIntegral`, `Ordering$IntOrdering` | `Integral<Object>` | `Integral[Int]` |
| `LongIsIntegral$` | `Numeric$LongIsIntegral`, `Ordering$LongOrdering` | `Integral<Object>` | `Integral[Long]` |
| `ByteIsIntegral$` | `Numeric$ByteIsIntegral`, `Ordering$ByteOrdering` | `Integral<Object>` | `Integral[Byte]` |
| `ShortIsIntegral$` | `Numeric$ShortIsIntegral`, `Ordering$ShortOrdering` | `Integral<Object>` | `Integral[Short]` |
| `CharIsIntegral$` | `Numeric$CharIsIntegral`, `Ordering$CharOrdering` | `Integral<Object>` | `Integral[Char]` (new) |
| `BigIntIsIntegral$` | `Numeric$BigIntIsIntegral`, `Ordering$BigIntOrdering` | `Integral<BigInt>` | `Integral[BigInt]` (new) |
| `DoubleIsFractional$` | `Numeric$DoubleIsFractional`, `Ordering$Double$IeeeOrdering` | `Fractional<Object>` | `Fractional[Double]` |
| `FloatIsFractional$` | `Numeric$FloatIsFractional`, `Ordering$Float$IeeeOrdering` | `Fractional<Object>` | `Fractional[Float]` (new) |
| `BigDecimalIsFractional$` | `Numeric$BigDecimalIsFractional`, `Ordering$BigDecimalOrdering` | `Numeric$BigDecimalIsConflicted`, `Fractional<BigDecimal>` | `Fractional[BigDecimal]` (new) |

"Which one actually gets picked as the implicit" is not determined by the jar's shape alone
(there are non-implicit siblings such as `BigDecimalAsIfIntegral` / `FloatAsIfIntegral`),
so I had real scalac print `implicitly[…].getClass.getName` and checked them one by one.

The implementation is confined to `crates/typer/src/prelude_numhier.rs`
(prepare `Integral` / `Fractional` in the prelude with a `<: Numeric[T]` edge, overwrite the
types `add_numeric` assigned, and add the missing instances).
The only change on the `prelude.rs` side is one line passing `library_abi` to the call.
`quot` / `rem` / `div` are supplied from the jar by `pickle_supply` and are not hand-written
(`Integral`'s type parameter has to be named `T`, the same as in the real library:
`pickle_supply` builds its scope by name, and with a different name it cannot map
`quot(T, T): T` across).

#### Why this is not ambiguous

Since `Numeric[T] extends Ordering[T]`, introducing `Integral[Int]` adds one more value that
conforms to `Ordering[Int]`. **Even so, the candidate set does not grow.** The implicit scope
of `Ordering[Int]` (SLS 7.2; `collect_type_parts` / `companion_implicits` in
`implicits.rs`) is `Ordering` and its base classes plus `Int`'s companion, and
**`Numeric`'s companion is not in it**. Real scalac likewise returns `Ordering$Int$` for
`implicitly[Ordering[Int]]`, not `Numeric$IntIsIntegral$`. The fixture `ig_hier.scala`
prints `implicitly[…].getClass.getName` for 13 cases and compares byte-for-byte against real
scalac, so it checks that we **pick the same thing real scalac does** rather than merely
asserting uniqueness.
`ambiguity_did_not_increase` in `crates/cli/tests/integral.rs` pins down that no
`ambiguous` appears for `Ordering[Int/Double/Long/Byte/Short/Char/Float]` or for `sum` /
`product` / `sorted` / `max` / `min` / `sorted` on tuples.
In slick too, the 8 `ambiguous` errors were **identical line for line**.

#### Prelude gaps closed along the way

- `Numeric[Float]` / `Numeric[BigDecimal]` (part of the 27 `no implicit` errors
  `agent/mismatch8` had reported).
- `Ordering.Option` (`implicit def Option[T](implicit ord: Ordering[T]):
  Ordering[Option[T]]`, which in the jar is
  `Ordering$.Option:(Lscala/math/Ordering;)Lscala/math/Ordering;`).
  `List(Some(2), None, Some(1)).sorted` now works.
  It is the same shape of gap as `Ordering.TupleN` (`prelude_ordtuple.rs`). In slick this
  removed two errors: `Ordering[Option[String]]` and the
  `Ordering[Tuple4[String, Option[String], Option[String], String]]` that has it as an
  element (`Ordering.Tuple4` was already there, but failed because its implicit argument
  `Ordering[Option[String]]` could not be filled).

#### The private runtime

`--no-scala-library` has neither the `scala/math/Integral` classfile nor
`Numeric$IntIsIntegral$`. To avoid emitting bytecode that references unloadable classes,
`prelude_numhier::install` **returns without doing anything** when not in `library_abi`
mode. `range_is_diagnosed_without_the_jar` pins down that compiling `ig_hier.scala` under
`--no-scala-library` produces `not found: type Integral` /
`range is not a member of List$`.

#### fixture

| fixture | what it checks | expected |
|---|---|---|
| `ig_hier.scala` | `range` on `List`/`Vector`/`Seq`/`Long`, the class names of 13 `implicitly` calls, `quot`/`rem`/`div`, user code taking a `Numeric[T]`, `sum`/`product`/`sorted`/`max`/`min`/`sortBy`, widening `Integral[Int]` → `Numeric[Int]` / `Ordering[Int]`, and `Ordering[Option[Int]]` | 42 lines (matching real scalac 2.13.16) |
| `ig_hier_bad.scala` (rejecting case) | Flowing backwards from `Numeric[Int]` → `Integral[Int]` and from `Ordering[Int]` → `Numeric[Int]`, the nonexistent `Integral[Double]` / `Fractional[Int]` / `Integral[String]`, and `List.range("a", "z")` | 6 compile errors (real scalac gives the same 6 on the same 6 lines) |

The measurement went from `files=184 errors=346 files_with_errors=64` to
**`files=184 errors=342 files_with_errors=64`** (`no implicit` 26 → 22).
The 4 that disappeared are `Numeric[Float]` / `Numeric[BigDecimal]` /
`Ordering[Option[String]]` /
`Ordering[Tuple4[String, Option[String], Option[String], String]]`, and
**not a single diagnostic was added** (the diff of `grep '^error' | sort | uniq -c` is just
those 4 deleted lines; the 8 `ambiguous` lines are identical line for line).

#### Remaining

- slick does not use `Integral` / `Fractional`, so all that went away were the 4
  `Numeric` / `Ordering` gaps. The remaining 22 `no implicit` errors are other type classes
  (`ClassTag`, cats, and so on).
- Writing `Ordering.by(...)` explicitly works. `Ordering.Iterable` is not in the implicit
  search, but `implicitly[Ordering[List[Int]]]` is **rejected by real scalac 2.13.16 too**
  (`Ordering` is invariant, so `Ordering[Iterable[Int]]` is not an `Ordering[List[Int]]`),
  so for now there is no difference.
- Writing `a + b` after `import Numeric.Implicits._` does not work
  (`+` resolves as `String` concatenation and gives `type mismatch`).
  The behaviour is the same before and after this fix, and the `n.plus(a, b)` form works.
- Naming a **non-implicit** instance such as `Numeric.BigDecimalAsIfIntegral` typechecks,
  but `pickle_supply` supplies it as a field of `Numeric$` and it fails at run time with
  `NoSuchFieldError: BigDecimalAsIfIntegral`
  (the correct form is `Numeric$BigDecimalAsIfIntegral$.MODULE$`).
  This is **the same before and after this fix** (it reproduces on binaries from before
  `agent/integral`) and is a problem with the shape of pickle-derived `object` members.
  The 9 that are selected as implicits are held as modules on the prelude side and are
  unaffected.
