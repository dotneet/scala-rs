# Typing arrays and collections

Development notes for the slices that worked on the type-checking side of
`Array`, `Set` / `Map`, and collection arguments in general. `Array` sits right
on the seam between the prelude's hand-written approximations, the pickle, and
erasure, so most of these bugs are about a shape that real scalac accepts and we
did not — a missing conversion, a type argument that could not be solved, or an
approximate signature that was subtly not nsc's.

(The *runtime* half of `Array` — the miscompilations that type-check fine and
break under `java` — lives in the codegen notes, not here.)

---

### `Array` is a type constructor, and a no-paren self call is still a tail call (`agent/asttype`)

Six errors in slick's `ast/Type.scala` and four in `compiler/RewriteJoins.scala`,
from five unrelated roots: `scala.Array`'s symbol carried no type parameter,
wildcards only get a kind inside type patterns, `@tailrec` never counted
`Select`-shaped self calls, `Ordering[Null]` needed both an inherited implicit
and `Predef.$conforms`, and mixin forwarders in Scala classfiles were shadowing
the pickle's declarations.

`tests/slick_measure.sh` goes **`errors=99 → 86`, `files_with_errors=39 → 36`**.
The two files went **6 → 0 and 4 → 0** with zero new errors (two errors in
`jdbc/JdbcBackend.scala` and one in `lifted/AbstractTable.scala` disappeared as
collateral). codegen was touched in exactly one place (item 5 below).

#### 1. `Array`'s kind is `* -> *`

Passing `Array` to `class TypedCollectionTypeConstructor[C[_]]`
(`implicit val forArray: TypedCollectionTypeConstructor[Array]`) compiles under
real scalac. We emitted
`kinds of the type arguments (Array) do not conform …`. `SymbolTable::kind_arity`
reads a class symbol's `tparams.len()`, but **`scala.Array`'s symbol has no type
parameters at all** — `Array[T]` in source becomes `Type::Array`, so there is
nowhere for `T` to be created. `class_tparam_count` now returns 1 for
`array_sym` specifically.

That makes substitution (`C := Array` into `C[E]`) produce
`Class { array_sym, [E] }`. That spelling already exists as a classfile-derived
one and is **the same type** as `Type::Array`. It is normalised at the entry of
`is_sub_type` via `array_class_form`, and the same conversion is applied in
`erasure::erase_ty` (without the latter the pseudo-name `[java/lang/Object` is
emitted as a class name and gives
`ClassFormatError: Illegal class name`).

#### 2. A wildcard takes on a kind **only inside a type pattern**

slick writes `case o: TypedCollectionTypeConstructor[?]`. nsc gives wildcards the
parameter's kind in type patterns, but in **ordinary type positions** it reads
the same `TC[_]` as "an existential over a proper type" and emits
`_$1 takes no type parameters, expected: 1` (confirmed with real scalac 2.13.16).
So the kind check is skipped only while `Checker::pattern_tpt` is set.
`def anyOf(t: TC[_])` is still diagnosed (`tests/fixtures/at_bad.scala`).

#### 3. A recursive call to a no-paren method is a `Select`, not an `Apply`

```scala
@tailrec
def sourceNominalType: NominalType = structuralView match {
  case n: NominalType => n.sourceNominalType
  case _              => this
}
```

`count_tailrec_calls` only recognised `Apply` / `TypeApply` as recursive calls.
A call to a method that takes no parameter list has no `Apply`, so we emitted
`could not optimize @tailrec annotated method: it contains no recursive calls`.
Only when the declaration has no `paramss` do we now also count `Select` /
`Ident` with a matching `sym` as calls. A non-tail position still gives
`a recursive call not in tail position` as before (`def loop: Int = loop + 1` was
previously misdiagnosed as "no recursive calls").

#### 4. `Ordering[Null]` is `Ordering.ordered` and `Predef.$conforms` combined

Real scalac's `-Xprint:typer` prints
`Ordering.ordered[Null](scala.Predef.$conforms[Null])`. We were failing to reach
both halves.

* `ordered` is declared in `trait LowPriorityOrderingImplicits`, which
  `object Ordering` **inherits**. `warm_pickled_implicits` only supplied a
  companion's **own** members from the pickle, so it was never in the implicit
  scope. It now walks parents (SLS 7.2's "members of the companion **object**"
  includes inherited ones). Priority is still decided by the existing
  `is_as_specific_origin` (the derivation relation between owners), so
  `Ordering[String]` still picks `Ordering.String`.
* `$conforms[A]: A => A` is added to `Predef` by `prelude_conform`, but that
  happens **after** `import_members(st, st.predef)` has pulled things into the
  base scope, so it can never be a scope-derived candidate. It is now offered as
  a candidate for **one-argument function types** only when every other candidate
  has failed (`Implicits::conforms_witness`). This also makes
  `implicitly[Ordering[java.util.Date]]` work.

#### 5. Mixin forwarders in Scala classfiles were hiding the pickle's declarations

`foundRefs.filter(_._2._2.isEmpty).map { … }` in
`RewriteJoins.hoistFilterFromBind` gave `value _2 is not a member of Any`.
`foundRefs` is an `immutable.HashMap`, and its classfile contains **forwarder
methods** with no `Signature` attribute:

```
public java.lang.Object filter(scala.Function1);
public scala.collection.IterableOps map(scala.Function1);
```

scalac always writes a `Signature` for a method whose Scala type touches a type
parameter, so **in a class with type parameters, a method without `Signature` is
a forwarder or a bridge**. Installing these as `(Any) => Any` next to the
parent's declarations hides the parent (`MapOps.filter`, which the pickle writes
properly). They are now skipped by `classpath::is_erased_scala_forwarder`,
leaving it to ordinary member lookup and `PickleSupply::complete`'s ancestor
path. This alone removed 13 `no matching overload` / `is not a member` errors in
slick (including three outside my assignment).

The bridge `sizeOf(Object)I` for a `def sizeOf(c: C[Int])` implemented at
`C = Array` needs a `checkcast [I`. `checkcast_internal` had no array arm and
`-Xverify:all` gave
`VerifyError: Type 'java/lang/Object' is not assignable to '[I'`, so one was
added.

#### Tests and fixtures

`crates/cli/tests/asttype.rs` (six tests). The fixtures are
`tests/fixtures/at.scala` (+ `expected/at.txt`), which collects every case in
one file, and the rejecting side `tests/fixtures/at_bad.scala`. `at.scala` uses
`@tailrec` / `Ordering` / `<:<` / `immutable.HashMap`, so it is library-ABI only,
and the test checks that `--no-scala-library` **diagnoses it**. Three of the six
fail on `main` before the fix.

#### Remaining

* Requiring **a function type itself** as an implicit argument, as in
  `implicitly[String => String]`, is diagnosed by
  `reject_unapplied_implicit_clause` as an "unfillable implicit clause" before
  search even begins, so it never reaches the `$conforms` of item 4 (indirect
  requests via `Ordering.ordered` do work).
* An `apply` given tuples of differing element types, like
  `immutable.HashMap("a" -> x, "b" -> y)`, gives `no matching overload` (no LUB
  is taken).
* Reading a `private[this] val` from an anonymous class in the same outer class
  gives an `IllegalAccessError`. nsc renames it to `O$$secret` and makes it
  public. This is a **pre-existing** codegen bug found during this work, and is
  independent of this slice.
* `@tailrec` still only checks; it does **not transform** the tail call (as
  before).

---

### Constructing and adding to `Set`/`Map`, and `Array` not being treated as a `Seq` (`agent/setmap`)

Eight collection-construction errors in slick were reduced to minimal
reproductions and split into **seven roots** — one missing `Array` wrapping,
several prelude approximations that fought with the real jar members, and a
couple of places where an undetermined type variable never got solved. One error
did not reach its root and simply moved one step upstream (see "Remaining").

slick goes `errors=44 files_with_errors=26` →
**`errors=37 files_with_errors=22`** (`tests/slick_measure.sh`; files that lost
errors: `ExpandTables.scala`, `PruneProjections.scala`, `QueryCompiler.scala`,
`ResultConverter.scala`). The fixture is `tests/fixtures/setmap1.scala` with all
cases in one file; the tests are in `crates/cli/tests/setmap.rs`. On main before
the fix (`61023ba`) that one file produces 13 errors.

**1. There was no wrapping to pass an `Array` as a `Seq`/`IndexedSeq`/`Iterable`.**
Even `def v(a: Array[Any]): Seq[Any] = a` did not compile. Real scalac's
`-Xprint:typer` prints:

```
def v(a: Array[Any]): Seq[Any]      = scala.Predef.copyArrayToImmutableIndexedSeq[Any](a)
def y(a: Array[Any]): Iterable[Any] = scala.Predef.genericWrapArray[Any](a)
```

`scala.Seq` / `scala.IndexedSeq` are aliases for the `immutable` ones, so the
`scala.collection.mutable.ArraySeq` returned by `genericWrapArray` does not
reach them and the lowest-priority `copyArrayToImmutableIndexedSeq`
(`LowPriorityImplicits2`) is chosen. `scala.Iterable` is
`scala.collection.Iterable`, which `genericWrapArray` does reach, so priority
picks that one. Both were added to `prelude_setmap.rs`, and
`seqfn_view.rs`'s `array_seq_wrap` (which handled only `Array[Boolean]`) was
generalised into `array_wrap_candidates`, choosing the first that fits in
priority order.

**The brief's reading — "`genericWrapArray` has an incompatible descriptor and
is unusable, so add `wrapRefArray`" — was wrong.** What is incompatible is the
`([Ljava/lang/Object;)` you get when you *declare* `Array[Any]`; with a genuine
type parameter, i.e. `Array[T]`, `erasure.rs`'s `array_elem_is_abstract`
collapses it to `Ljava/lang/Object;` exactly as nsc does (javap:
`public <T> scala.collection.mutable.ArraySeq<T> genericWrapArray(java.lang.Object)`).
`wrapRefArray` is constrained to `T <: AnyRef` and does not apply to
`Array[Any]`, which is why nsc does not pick it there either.

For the same reason as `wrapBooleanArray`, these are **not made `implicit`**
(making them implicit would compete with `refArrayOps` in ordinary member
selection on an `Array`). On the overload-resolution side one more entry was
added to `arg_conforms`'s list of views (this is the path by which
`TupleSupport.buildTuple(a)` reaches an `IndexedSeq[Any]` parameter).

**2. `scala.collection.Map` had no members at all.** The
`scala/collection/Map` built by `prelude_hier.rs`'s `LINKS` is a link with type
parameters only, and `pickle_supply::adopt_binary_class` does not touch prelude
classes with `scala/` names (`class_sym.0 < st.prelude_end`), so it is never
supplemented from the jar either. slick's `expansions contains tsym` gave
`not a member`, and `expansions(tsym)` fell through to the **companion's
varargs `apply`**, giving
`no matching overload for ((K, V)*)Map[K, V]`. The three reads on
`collection.MapOps` (`contains` / `apply` / `get`) were declared.

**3. The prelude's approximate members were competing with the jar's real ones.**
`prelude_coll.rs` hand-writes `Set.map(A => Any): Set[Any]` and
`Map.+((K, Any)): Map[K, Any]`. `immutable.HashSet` / `HashMap` reach **both** —
upward to the pickle's `IterableOps` / `MapOps`, sideways to the prelude's `Set`
/ `Map` — and neither owner is a subclass of the other, while `A => B` conforms
to `A => Any` and `map[B]` can be applied with `B = Any`, so both `HashSet.map(f)`
and `HashMap + kv` were `ambiguous overload`. nsc sees exactly one member, and it
is the jar's. Only on ambiguity, the side with a `pickled_origin` is kept.

**4. Member selection did not substitute through an element type annotated
`@uncheckedVariance`.** Calling `.map(_._1)` on the result of slick's
`ConstArray.toSet: immutable.HashSet[T @uncheckedVariance]` returned `_1` as the
`T1` declared on `Tuple2`, so `referenced.map(_._1)` came out as `HashSet[T1]`.
`Type::Tuple` is not handled by `subst_as_seen_from` (`type_select`'s
`subst_args` is the list that exists for that), so the annotation is now stripped
before looking. A type annotation says nothing about members.

**5. `Option` was not an `IterableOnce`.** In 2.13 it is
`sealed abstract class Option[+A] extends IterableOnce[A]` (a real parent, not
2.12's `option2Iterable`). Real scalac passes it straight to
`Set.apply[String]().++(o)` **with no conversion**.

**6. `++` is two overloads.** javap:

```
scala.collection.SetOps:      public default C    $plus$plus(scala.collection.IterableOnce<A>);
scala.collection.IterableOps: public default <B> CC $plus$plus(scala.collection.IterableOnce<B>);
```

The prelude side had only one, corresponding to the former (created by
`prelude_coll` and widened by `prelude_buildfrom::widen_set_concat`), and since
`lookup_member` finds it, the pickle side is never asked for `++` at all
(confirmed with `SCALA_RS_PICKLE_DEBUG=1`; `concat` *is* asked for, so it has
both). Hence `s ++ anOptionOfSomethingElse` was `no matching overload`. The
polymorphic version was added in `prelude_setmap.rs`, along with two rules:

* The key for `pickle_supply`'s "only one declaration per erased argument list"
  gained a component for **whether it has its own type parameters**. What that
  rule protects is "two that differ only in result type"
  (`IterableOps.map[B]` and `MapOps.map[K2, V2]`), and since both are
  polymorphic they still collapse to one. A pair where one is monomorphic is a
  genuine overload distinguishable by its arguments.
* When a monomorphic and a polymorphic alternative come out **equally specific**,
  the monomorphic one wins. With `Set()`'s element type undetermined,
  `IterableOnce[?A]` and `IterableOnce[B]` accept each other, but nsc picks the
  monomorphic one (`-Xprint:typer` prints `.++(o)` with no type argument).

**7. Solve an empty factory's type argument from the following argument.**
`Set()` is left as `Set[?A]` (deliberately, by `instantiate_leftover_tparams`),
and `++`'s argument was supposed to solve it, but `undet_compatible` only looked
at variables held by the **argument** side. The case where the **parameter** side
is undetermined was added to `arg_score`. The substitution after solving already
exists on the `OverloadPick::Found` side. When a wrapping is interposed, as in
`Map() ++ arrayOfPairs`, unification has to happen on the wrapped type, so one
line was added there too.

Note that adding the polymorphic `++` of item 6 **newly broke two** occurrences
of `oldDiscCandidates ++ (tree match { … case _ => Set.empty })` (slick
`ExpandSums.scala`). Because it became overloaded, `proto_arg_type` stopped
passing a prototype to the argument, so the `match` arms were lubbed with no
expected type into the existential `Set[_ <: AnyRef]` (**real scalac also
produces an existential for the same expression with no expected type**; the
reason nsc does not suffer here is that it fits each arm against
`IterableOnce[A]` as the expected type). It was restored by a rule
(`only_concrete_param`): when exactly one alternative in the overload set has a
concrete type at that argument position, use it as the prototype.

**Remaining (with minimal reproductions)**

* `m.Column(name=…, options = Set() ++ … )` (`JdbcModelBuilder.scala:279`). `++`
  now works, but the expected type `Set[ColumnOption[_]]` does not reach `Set()`,
  so it becomes `Set[ColumnOption[Nothing]]` and fails one step upstream with
  `no matching overload for Column$`. The error just moved from line 280 to line
  279; the file count did not change. The root is
  `proto_arg_type`'s `!type_mentions_wildcard(p)`: a parameter containing a
  wildcard is not used as a prototype. Removing that makes the reproduction below
  compile, but **slick's numbers did not move by one** (`m.Column` goes through
  the `ModuleRef` path via the companion's `apply`), so a widening with no
  measurable benefit was abandoned.

  ```scala
  sealed trait CO[+T]
  case class SqlType(s: String) extends CO[String]
  case object AutoInc extends CO[Nothing]
  object S {
    def take(options: Set[CO[_]]): Int = options.size
    def a(d: Option[String], ai: Boolean): Int =
      take(Set() ++ d.map(s => SqlType(s)) ++ (if (ai) Some(AutoInc) else None))
  }
  ```

* `session.withPreparedInsertStatement(sql, keyColumns.toArray)`
  (`JdbcActionComponent.scala:725`) turned out to be **someone else's root**.
  `ConstArray.toArray` is `def toArray[R >: T : ClassTag]: Array[R]`, and `R`,
  which has only a lower bound, stays undetermined as `Array[R]`, so it matches
  both the `Array[String]` and `Array[Int]` versions and is `ambiguous`. nsc
  drops to the lower bound and gets `Array[String]`. Minimal reproduction:

  ```scala
  import scala.reflect.ClassTag
  class CA[+T](val xs: Seq[T]) { def toArray[R >: T : ClassTag]: Array[R] = xs.toArray[R] }
  object G {
    def over[T](sql: String, names: Array[String] = new Array[String](0))(f: Int => T): T = f(1)
    def over[T](sql: String, idx: Array[Int])(f: Int => T): T = f(2)
    def call(ca: CA[String]): Int = over("x", ca.toArray)(_ + 1)   // ambiguous overload
  }
  ```

  The `xs.toArray[R]` (explicit type argument) in the same file also gives
  `found: Array[T] required: Array[R]`. That is the same root as the "explicit
  type arguments do not go through as-seen-from on a generic parent's member"
  item below.

* `scope + (sym -> el)` at `Node.scala:534` is downstream of **the `:@` extractor
  not being found** (line 533) and is not this slice's business.

**Other things found along the way (not fixed here)**

* A member call written with explicit type arguments does not go through
  as-seen-from. `s.map[Int](_.length)` (with `s: immutable.HashSet[String]`)
  gives `value length is not a member of A` plus
  `found: CC[Int] required: HashSet[Int]`. Without the type argument,
  `s.map(_.length)` works.
* In a file containing `Array[Any](1, "a")`, an `Array(3, 1, 2)` that appears
  **later** and infers its element type emits a broken descriptor (it picks
  `Array$.apply(Int, Seq[Int])` and then calls `apply(Seq, ClassTag)`, giving a
  `VerifyError`). Writing `Array[Int](3, 1, 2)` avoids it. Pre-existing on main
  (`61023ba`).

  ```scala
  object Main {
    def a(): Unit = println(Array[Any](1, "a").mkString(","))
    def b(): Unit = println(Array(3, 1, 2).sum)      // VerifyError
    def main(args: Array[String]): Unit = { a(); b() }
  }
  ```

* `Array[(Int, String)](1 -> "one")` creates an `Object[]` and then `checkcast`s
  it to `[Lscala/Tuple2;`, giving a `ClassCastException`. Also pre-existing on
  main; the fixture avoids it by building the array with element assignment.

---

### Seven collection-argument errors, seven roots (`agent/final1`)

The seven remaining "passing a collection as an argument" errors in slick were
reduced **one at a time**, and produced **seven errors, seven roots** — matching
the running observation that neither the same symptom nor the same file implies a
single root. All of them were checked against real scalac 2.13.16 (both what it
accepts and what it rejects) before being fixed.

slick goes `errors=17 files_with_errors=13` →
**`errors=10 files_with_errors=8`** (`tests/slick_measure.sh`; zero new errors;
files that lost errors: `util/ConstArray.scala`, `jdbc/JdbcModelBuilder.scala`,
`jdbc/JdbcActionComponent.scala`, `compiler/ExpandSums.scala`,
`compiler/MergeToComprehensions.scala`).

The fixture is `tests/fixtures/final1.scala` with all cases in one file (plus the
rejecting side `final1_bad.scala`); the tests are in
`crates/cli/tests/final1.rs`. On main before the fix (`d7e7767`) this one file
produces 12 errors.

**1. `apply` could not be inserted on a self alias `self =>`.**
`def apply(idx: Int) = self(idx)` in
`final class ConstArray[+T](a: Array[Any], val length: Int) { self => … }` gave
`value apply is not a member of ConstArray.this.type`. The type of `self` is
`C.this.type` (`Type::ThisType`); the `Select` side widened it to the class to
look up members, but **`resolve_overload` had no `ThisType` arm** and stopped at
`_ => None`. It now re-reads it as a `Type::Class` with the class's own type
arguments and delegates to the `Class` arm.

**2. A type parameter with only a lower bound was not settled before the implicit
clause.** `session.withPreparedInsertStatement(sql, keyColumns.toArray)` gave
`ambiguous overload … with arguments (String, Array[R])`. The `R` of
`ConstArray#toArray[R >: T : ClassTag]: Array[R]` stayed undetermined as
`Array[R]` and matched both `(String, Array[String])` and `(String, Array[Int])`.

In `adaptToImplicitMethod`, nsc runs
`inferExprInstance(..., keepNothings = false)` **before** looking for the
implicit clause. Variables that would become `Nothing` are left open (this is why
`take(Array.empty)` gets decided from the argument side), but variables with a
real lower bound are **settled at that bound**. That is
`solve_lower_bounded_undet`, and the lower bound used is not the declared one but
**the one as seen from the receiver** (the `T` of `R >: T` is `String` for a
`ConstArray[String]`).

The `adapt_implicit_apply` side needed care too. It has a rule that anything
"which has type parameters but is not a `TypeApply`" bails out waiting for a
witness, and that was stopping even the `(ClassTag[String])Array[String]` from
which `R` had already vanished. But "the current type does not mention the
parameter" is not enough on its own — unlike `type_mentions_wildcard`,
`type_mentions_tparam` **does not look inside compound types**, so slick's
`BaseColumnType[U] = ScalaType[U] with BaseTypedType[U]` reads as "mentions
nothing" and implicit search runs with `U` unsubstituted (the `ovl4` fixture
fails). It is now let through **only when comparing the declared type against the
current type shows a substitution actually happened**.

**3. The "typing call arguments" flag leaked into lazy signature completion.**
The fourth argument of
`m.Table(namer.qualifiedName, columns, primaryKey, buildForeignKeys(builders), indices)`
came out as an **unapplied method type**
`((Option[ForeignKey]) => IterableOnce[B])Seq[B]`.

`typing_call_args` marks "this expression is an argument whose target is not yet
settled", and `adapt_implicit_apply` uses it as a condition for leaving implicit
clauses alone. But it is **a flag on the typer, not on the expression**, and lazy
signature completion running from the middle of an argument inherited it
verbatim. As a result the implicit clause (`A => IterableOnce[B]`) of the
`.flatten` in the forward-referenced

```scala
final def buildForeignKeys(builders: Builders) =
  mForeignKeys.map(mf => createForeignKeyBuilder(this, mf).buildModel(builders)).flatten
```

was never filled, and that became **the method's inferred result type itself**.
The giveaway is that writing the same definition above its use works. The flag is
now cleared for the duration of `type_def_body`'s typing of the body.
`m.Model(… .map(_.buildModel(builders)))` at `JdbcModelBuilder.scala:93` was a
cascade of this and disappeared with it.

**4. Undetermined variables introduced by an argument were not minimised before
the join.** `tableFields.getOrElse(t.identity, Seq.empty)` became `Seq[AnyRef]`,
which made the downstream `f` an `AnyRef` and gave
`found: Some[(TableNode, ConstArray[((TypeSymbol, AnyRef), List[AnyRef])])]`.

The `V1` of `getOrElse[V1 >: V]` is the join of "the declared lower bound
`Vector[TermSymbol]`" and "the type of the argument `Seq.empty`". The `A` of
`Seq.empty` was still an undetermined variable, and
`lub(Vector[TermSymbol], Seq[?A])` walked base types until both met at `Seq` and
joined the arguments to give `Seq[AnyRef]`. nsc minimises a variable with no
upper constraint to its lower bound (`Nothing` by default) before joining, giving
`Seq[TermSymbol]`. `minimize_undet` was inserted into both
`unify_tparam_all`'s join and the join against the declared lower bound.

**5. A constructor pattern was being applied to a non-case class.** In
`case IfThenElse(ConstArray(Library.Not(…), ProductNode(ConstArray(Disc1, map)), …))`
the `map` came out as `Int` rather than `Node`, `disc` as `Array[Any]`, and
`ProductNode(ConstArray(disc, map))` as `ConstArray[Any]`.

By SLS 8.1.6/8.1.7 only **case classes** have constructor patterns.
`ConstArray` is `final class ConstArray[+T](a: Array[Any], val length: Int)` with
an `unapplySeq` on its companion. We were checking "if `ctor_fields` is non-empty
and the arity matches, it is a constructor" first, so it bound the two fields
`a: Array[Any]` and `length: Int`. Now, if there is an extractor the extractor is
used, and the `ctor_fields`-only arm is left for **classes with no extractor**
(the case it was needed for).

**6. An expected type had no effect on undetermined variables introduced by the
receiver.**
`def sqlOptions(dbType: Option[String]): Set[ColumnOption[_]] = Set() ++ dbType.map(SqlType(_))`
came out as `Set[SqlType]`, and the **invariant** `Set` would not accept the
expected type. The `?A` of `Set()` was read as `SqlType` from the argument and
never revisited, even though the `?A` of `Set[?A]` sits in an invariant position
in the result. `add_expected_constraints` already does this for the callee's own
type parameters (nsc's `instantiateExpecting`). The same rule was extended to
receiver-derived variables, **only in invariant positions and only when the
argument's solution conforms to the expected type**.

**7. A conversion search with nothing to solve was passing on shape-only
unification.** Even with 6 fixed, the chain
`Set() ++ … ++ (if(!autoInc && !generated) convenientDefault else None)` stayed
`Set[ColumnOption[Nothing]]`. For the last `++`'s argument of type
`Option[Default[_]]`, **`Option.option2Iterable` was claiming to be a view to
`IterableOnce[ColumnOption[Nothing]]`**. Once that goes through, the monomorphic
`Set#++(IterableOnce[A]): Set[A]` (the prelude's widened one) becomes applicable,
and having no type parameters it leaves no room for the expected type to override
anything.

The root is in `open_conversion_fit`: when **no variables remain to be solved on
either the candidate side or the call site**, it still let `Unify` make the
decision. To `Unify` a wildcard matches anything, so `Iterable[Default[_]]`
"matched" `IterableOnce[ColumnOption[Nothing]]`. When there is nothing to solve,
it now asks about conformance directly (`is_sub_type`). Real scalac does not
accept this view either, and rejects all three shapes (the `w2`/`w5`/`x2`
equivalents) that pass an `Option[Default[_]]` where an
`IterableOnce[ColumnOption[Nothing]]` is wanted.

**Checking against the brief's readings.** None of the three inherited
hypotheses were correct.

* "The root of the `Column$` case is `proto_arg_type`'s
  `!type_mentions_wildcard(p)`; look at the `ModuleRef` path" — **no**. Removing
  that exclusion and passing "a concrete argument type on which all alternatives
  agree" down the `ModuleRef` path did not move the numbers (the improvement is
  correct in itself and was kept). The same call fails **even with no wildcard
  anywhere** (`Set[ColumnOption[String]]` behaves identically). The roots are 6
  and 7 above.
* "`toArray` just needs to be dropped to its lower bound" — the direction is
  right, but **the lower bound alone is not enough**. The substance is the
  *timing*: it has to be dropped before the implicit clause is searched for.
* "The `Table$` case is a leftover of the same kind of root as `agent/implclause`"
  — **a different root**. The symptom of a surviving implicit clause is the same,
  but the cause is `typing_call_args` leaking into lazy completion, unrelated to
  the four `implclause` fixed.
* `JdbcModelBuilder.scala:93` was a cascade of 159 (this one was right).

**Other things found along the way (not fixed here)**

* `val x = ca.toArray` with no expected type (`toArray[R >: T : ClassTag]`) makes
  `(ClassTag[R])Array[R]` the value's type, implicit clause and all. Real scalac
  gives `Array[String]`. Item 2 was scoped to argument position only, so `val`
  initialisers are unchanged.
* A class with a method containing `new Array[R](len)` for an abstract `R`
  writes the pseudo class name `[java/lang/Object` into the constant pool and
  gives a `ClassFormatError` (type checking passes, and it does not show up in
  slick's measurements). The fixture avoids it with `Array.tabulate[R]`. It is a
  hole on the codegen side.

  ```scala
  final class Holder[+T](a: Array[Any], val length: Int) {
    def toArray[R >: T : ClassTag]: Array[R] = {
      val ar = new Array[R](length)   // ClassFormatError: Illegal class name "[java/lang/Object"
      ar
    }
  }
  ```

* A `toArray[R]` written with an **explicit type argument**, as in
  `(0 until n).map(f).toSeq.toArray[R]`, comes out as `Array[T]` and gives
  `found: Array[T] required: Array[R]`. Same shape as the "member calls with
  explicit type arguments do not go through as-seen-from" item recorded by
  `agent/setmap`.
* `val y = Seq.empty` stays `Seq[A]` (where `A` is `Seq.empty`'s type parameter),
  so `val z: Seq[Nothing] = y` gives `found: Seq[A]`. A side effect of the design
  that leaves undetermined variables in a value's type; item 4's minimisation was
  scoped to argument position only.
