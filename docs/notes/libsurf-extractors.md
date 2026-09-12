# The extractor protocol and the library surface (agent/libsurf)

What this slice changed, and what it deliberately left alone. Every claim
below was checked against scalac 2.13.16 (`/tmp/scala-2.13.16/bin/scalac`)
with the real library jar, running both compilers' output under
`java -Xverify:all`.

## Name-based pattern matching (SLS 8.1.8)

An `unapply` may return `Boolean`, or **any** type with a nullary
`isEmpty: Boolean` and a nullary `get`; `Option` is only the common case.
One sub-pattern matches the whole `get` value, several match its product
selectors `_1` … `_N`. scala-rs read the extracted types off `Option`'s type
argument and the backend always called `scala.Option.isEmpty` / `get`, so an
extractor returning its own class failed verification, `case Foo997(a)` on an
`Option[(String, String)]` was reported as an arity error, and
`case Extractor(x, y)` on an `Option[Product2[Int, String]]` cast the product
to `Tuple2`.

* `crates/typer/src/name_based.rs` decides the sub-pattern types from the
  number of sub-patterns, resolves `isEmpty` / `get` (discarding
  parameter-taking overloads, reading them at the result's own type
  arguments), and records the members for the backend
  (`SymbolTable::name_based_unapply`, `unapply_selectors`).
* `crates/backend/src/gen_match.rs` emits the calls as ordinary member
  selections on a synthetic local, so a value-class result goes through its
  `$extension` statics and an interface member through the right dispatch.
  A value-class local holds the *underlying* value, so the synthetic receiver
  carries the erased type (`erased_receiver_ty`); typed at the value class
  itself, the read unboxed it a second time.
* Rejections scalac also makes: a non-`Boolean` `isEmpty`, a missing `get`,
  and an `unapply` that does not take exactly one first-clause argument
  (`neg/t5078`, judged only after the signature is complete -- a local case
  class's synthetic `unapply` is typed lazily, and `cats`' `case class
  Deferred` inside a method body is matched as a constructor pattern).
* One sub-pattern for a *declared* tuple keeps nsc's deprecation
  (`neg/t6675`); `pos/t6675` shows the declaration, not the instantiation,
  is what counts.

## Implicit clauses after the scrutinee

`def unapply(s: String)(implicit p: Option[String] = None)` (`run/t3353`) and
`def unapply[S, T](s: S)(implicit w: FooHasType[S, T])` (`run/t6111`) were
called with the scrutinee alone, which did not verify. The typer now types
the call `X.unapply(<selector>)` in full (`type_unapply_call`), which also
solves a type parameter only the implicit mentions, and the implicit
arguments ride along as `Apply(fun, implicits)` inside the `UnApply`, which
the backend passes after the scrutinee.

## What a collection transformation returns

2.13 declares every transformation on `IterableOps[A, CC, C]` to return `C`
or `CC[B]`. The `BuildFrom` rebuild put the *receiver's own class* back,
which is wrong wherever a class binds `CC` / `C` to something else, and the
`checkcast` codegen then threw `ClassCastException`:

```scala
Vector(1,2,3,4).view.filter(_ % 2 == 0)   // View[Int], not IndexedSeqView
(1L to 5L).map(_ * 2)                     // IndexedSeq[Long], not NumericRange
m.view.mapValues(-_).drop(1)              // View, not MapView
```

`crates/typer/src/ops_shape.rs` reads the `CC` and `C` the receiver's own
pickle passes to `IterableOps` (walking its linearization) and rebuilds a
`C`-returning member to `C` and a `CC[B]`-returning one to `CC`. A map's
`CC` is `MapOps`' own, so only the `C` slot is read for a receiver with more
than one type parameter. A collection outside the library binds `CC` through
the parent it extends, so `collect` is not rebuilt to a user class at all.
A declared result that is already more specific than the receiver's `CC` is
kept (`SortedSetOps.collect[B](pf)(implicit Ordering[B]): SortedSet[B]`).

`aSortedSet.map(f)` resolved to the prelude's crude `immutable.Set.map`,
built a `HashSet` where nsc builds a `TreeSet`, and was then narrowed back to
`SortedSet` -- a `ClassCastException`. The prelude leaves the `…Ops` traits
out of its hierarchy, so the class-file walk never met
`SortedSetOps.map[B](f)(implicit Ordering[B])`;
`check_types::pickle_declares_other_arity` now consults the receiver's
pickled declarations (`PickleSupply::pickled_arities`). `BitSet.map(f: Int =>
Int): BitSet` shows why a pickled copy must stand for that very declaration:
once `SortedSet` has its copies, a `BitSet` inherits them.

## Smaller library-surface repairs

* `map` on `Option` / `Try` / `Either` was declared `A => Any`, so lambda
  bodies were typed against `Any`; `Try[Int](throw e)` ignored its explicit
  type argument (`crates/typer/src/prelude_polymap.rs`).
* `TupleN extends ProductN[T1, …]` (`run/unapply`, `pos/unapplySeq`).
* A `Unit` scrutinee: the extractor's parameter takes `BoxedUnit.UNIT`
  (`run/t9029`).
* An instance extractor whose static type is a trait over the class that
  declares `unapply` (`Date.unanchored`, an `UnanchoredRegex`) needs the
  receiver cast (`tests/rtprobe/gap_regex_unanchored.scala`).
* `StringContext(..).s(args)` written out as a call: `s` is a macro and the
  jar has no `s(Seq)` method, so it became `NoSuchMethodError`. It is
  rewritten as nsc expands it, `sc.standardInterpolator(processEscapes,
  args)` (`run/interpolationArgs`).
* `java.util.Collections.emptyIterator[String]()`: a zero-argument Java
  method is installed with no parameter clause, so the `TypeApply` callee
  path auto-applied it and the `()` became an `apply()` on the result.

## Not fixed

### Name-based `unapplySeq` with a product prefix

```scala
object FooSeq { def unapplySeq(x: Any): Option[Product2[Int, Seq[String]]] = … }
b match { case FooSeq(s: Int, _, n: String) => … }   // pos/unapplySeq
```

`_1` binds the first sub-pattern and the elements of the trailing sequence
bind the rest; the sequence may also be a sequence-*like* value with
`lengthCompare` / `apply` / `drop` / `toSeq` (`run/string-extractor`'s
`StringExtract`, `run/value-class-extractor-seq`'s `Array.UnapplySeqWrapper`).
scala-rs reads the whole `get` value as the sequence. Typing it without the
matching codegen would be a stub, so both halves are left for one slice:
the typer needs the product-prefix element types, and
`gen_unapply_seq_bind` / `gen_unapply_wrapper_bind` need to read the prefix
selectors and then walk the last component. `neg/t11102` is the same root
(scalac reports its own internal error there).

### `View.flatMap` with an `Option`-returning function (`run/view-headoption`)

```scala
val f0 = List(failer, succeeder).view flatMap (f => f())   // f(): Option[Int]
f0.head                                                    // CCE: Integer -> Option
```

`check_apply`'s `flatMap` branch takes the lambda's whole result as the
collection when `B` is still open, so the element type became `Option[Int]`
instead of `Int` (nsc gets `Int` through `option2Iterable`).

### Belongs to other slices

* `run/value-class-extractor-2`: a match joins `Opt.None` (the boxed value
  class) with `Opt("…")` (the erased `String`) inside `ValueOpt$.unapply`
  itself -- value-class erasure in branch joins (agent/erascg).
* `run/virtpatmat_unapplyprod`, `run/virtpatmat_unapplyseq`,
  `run/unapply`: our program output matches scalac's; the check files also
  hold scalac's exhaustivity / `Null` lint warnings (agent/warn).
* A user `Seq` subclass's `tail` needs `scala$collection$SeqOps$$super$
  sizeCompare`, which we do not emit (`AbstractMethodError`, agent/mixcg).

## The five prelude items agent/erascg reported

1. **`Set.toSeq` / `Map.toSeq` declared `List`** -- fixed
   (`crates/typer/src/prelude_toseq.rs`). `IterableOnceOps.toSeq` is
   `immutable.Seq[A]` and an `immutable.Seq` returns `this.type`, so the
   prelude's `List` made the call name a descriptor the library does not have
   and the *use* of the result failed verification. `run/t3563` passes.
2. **`Map.empty ++ seq` after a `Set` operation** -- already correct on
   batch/w2 (agent/erascg's own merge); `run/t2417` passes, and the `n7.scala`
   repro now agrees with scalac.
3. **`1.toByte.to(3.toByte)` is a `NumericRange`, scalac's is a `Range`** --
   **not** fixed, and withholding the members is *not* the fix. The prelude
   declares `to` / `until` on `RichByte` / `RichShort`, which the library's
   `ScalaWholeNumberProxy` does not have; nsc widens the receiver to `Int` and
   uses `RichInt.to`. Removing them (tried, reverted) turns valid programs
   into "value to is not a member of Byte": our implicit-view search does not
   widen a numeric receiver. That widening is the real repair, and it belongs
   with whoever owns numeric view resolution; the prelude's `NumericRange`
   declarations can go once it exists.
4. **`Regex.pattern` / `UnanchoredRegex.pattern` "not a member"** -- root
   found, not fixed. The pickle supply declines the member outright:
   `scala/util/matching/Regex#pattern: unmappable result type Ref {
   sym: "java.util.regex.Pattern" }` (`SCALA_RS_PICKLE_DEBUG=1`). A pickled
   type naming a *Java* class has no Scala pickle to read, and `conv` gives up
   instead of stubbing the class from the classpath
   (`classpath::find_or_stub_java_class`). Every library member whose
   signature mentions a JDK type is lost the same way, so the fix is worth
   making in `pickle_supply::conv_ref` -- in the supply seam, needing the seam
   test list.
5. **`4.byteValue` / `4.doubleValue` "not a member"** -- root found, not
   fixed, and it is wider than reported: `shortValue`, `longValue`,
   `floatValue` and `byteValue`/`doubleValue` all fail, only `intValue`
   resolves. The trace shows the member *is* supplied
   (`scala.runtime.RichInt#byteValue: supplied 1 overload(s)`, from
   `ScalaNumericAnyConversions` through RichInt's pickled parents) yet the
   selection on the `Int` receiver still fails, and RichInt is also stubbed as
   a module in the same run -- so the supplied member is not where the view
   application looks for it.
