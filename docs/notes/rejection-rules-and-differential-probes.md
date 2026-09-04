# Rejection rules, subtyping corners, and differential probing

Development notes for two related kinds of work. First, the "rules that reject":
variance checking, self-type conformance, wildcard bounds, lubs — where a bug
shows up as a **false positive** on code real scalac accepts. Second,
differential probing: writing ordinary programs, compiling them with both real
scalac 2.13.16 and scala-rs, running both, and comparing stdout byte for byte —
the only way to find miscompilations that type-check cleanly.

---

### 11 false positives emitted by the rules that reject (`agent/reject`)

Variance checking (SLS 4.5) and self-type conformance checking are both "rules
that reject". slick compiles completely under real scalac 2.13.16, so all 11
errors were false positives — and the seven variance errors turned out to be
**one root**, the four self-type errors **one root**.

`tests/slick_measure.sh` goes **`errors=65 → 54`, `files_with_errors=34 → 29`**.
The 11 that disappeared are exactly my assignment (7 variance, 4 self-type
conformance), with **zero new errors** (the set difference of `grep '^error'` is
11 deleted lines and nothing else). codegen (`crates/backend/`) was untouched, so
`tests/slick_subset.sh` was skipped.

**The symptoms count differently from the brief**, though: `covariant` is four,
not two (`head` / `headOption` in `BasicProfile.scala` give two, and
`overrideStatements` in `SqlProfile.scala` gives two, for `R` and `S`), which
with three `contravariant` makes **seven variance errors**; self-type is four
(two in `JdbcBackend.scala`: a named class and the anonymous class
`new JdbcDatabaseDef[F](…){}`), for 11 in total.

And there were **not as many roots as symptoms**. The seven variance errors are
one root, the four self-type errors one root: two roots in all. That is the other
side of "the same symptom is not necessarily one root".

**1. Which position a type argument stands in was only read from classes**
(7 variance errors).

`check_variance_ty` only read the declared variance (`+` / `-`) of `sym`'s type
parameters, and so only flipped positions, for `Type::Class { sym, args }`; for
`Type::Applied { ctor, args }` — the application of a **non-class** type
constructor — it treated all arguments uniformly as **invariant positions**. nsc
reads `sym.typeParams` whatever the head is. Whether the head is an **abstract
type member** or a **higher-kinded type parameter**, the declared variance
applies exactly as it does for a class. In slick's

```scala
trait BasicAction[+R, +S <: NoStream, -E <: Effect] extends DatabaseAction[R, S, E] {
  type ResultAction[+R, +S <: NoStream, -E <: Effect] <: BasicAction[R, S, E]
}
trait BasicStreamingAction[+R, +T, -E <: Effect] extends BasicAction[R, Streaming[T], E] {
  def head: ResultAction[T, NoStream, E]
}
```

the `ResultAction[T, NoStream, E]` has its first argument declared `+`, so the
covariant `T` is in covariant position, and its third declared `-`, so the
contravariant `E` is in contravariant position; both are legal. Treating them as
invariant produces two errors from the single `head`
(`covariant type T …` and `contravariant type E …`), and together with
`headOption` and `SqlAction.overrideStatements` that makes seven.
`tparam_variances`, which reads variance from the `tparams` of
`Type::TypeMember` / `Type::TypeParam` / a partially applied `Type::Class`, was
added and is used in the `Applied` arm (`crates/typer/src/check.rs`).

**That this is not too permissive** was checked on the rejecting side. An
unannotated `type M[X]` stays invariant
(`covariant type A occurs in invariant position`), a `type N[-X]` **flips** the
position (`… occurs in contravariant position`), and the higher-kinded
parameters `F[X]` / `G[-Y]` behave the same — four shapes that fail with the same
four errors as real scalac (`tests/fixtures/rej_bad.scala`).

**2. Self types were compared against a bare class with its type arguments
dropped, still phrased in the declaring vocabulary** (4 self-type errors).

`check_self_conformance` built the thing being checked as
`Type::Class { sym, args: vec![] }` — **type arguments dropped** — and used the
parent's `self_type` **verbatim** as the other side. A self type is written in
the vocabulary of the trait that declared it, so reading it here needs two
translations.

- The parent's type parameters. The `F` of `this: Database[F] =>` is
  `BasicDatabaseDef`'s `F`, not `JdbcDatabaseDef`'s.
- **Abstract type members** that the enclosing cake later aliased. `Database` is
  `BasicBackend`'s `type Database[F[_]] >: Null <: BasicDatabaseDef[F]`, and
  inside `JdbcBackend` it is `type Database[F[_]] = JdbcDatabaseDef[F]`.

```scala
trait BasicBackend {
  type Database[F[_]] >: Null <: BasicDatabaseDef[F]
  trait BasicDatabaseDef[F[_]] extends AnyDatabaseDef { this: Database[F] => … }
}
trait JdbcBackend extends RelationalBackend {
  type Database[F[_]] = JdbcDatabaseDef[F]
  abstract class JdbcDatabaseDef[F[_]](…) extends BasicDatabaseDef[F] { … }
}
```

Without the translations, what is being compared is "bare `JdbcDatabaseDef`"
against "`BasicBackend.Database[F]`", and **nothing can conform to that**. Which
is why the three classes `JdbcBackend` / `HeapBackend` / `DistributedBackend`
and the anonymous class `new JdbcDatabaseDef[F](…){}` all failed together.
`self_type_of_class` now supplies the type arguments, `subst_as_seen_from`
resolves the parent's type parameters, and `expand_type_members` resolves the
enclosing class's aliases. `expand_type_members` walks `enclosing_classes` from
the inside out, so an anonymous class reaches the same alias via
`object JdbcBackend`.

Here too the rejecting side still works. With the cake's alias being `Real[F]`,
`class Fake[F[_]] extends DbDef[F]` fails (as it does under real scalac), and so
does `class Miss[A] extends P[A]` against `trait P[A] { self: Q[A] => }`. main
before the fix was failing even `Real[F]` **itself** here (7 errors), so the
rejecting-side tests check not only "that it fails" but also **how many times**.

**3. A third thing found along the way**: `subst_as_seen_from` walked a class's
**parents** but not its **self type**. A self type is the other route by which
`this` inherits members, so the types of members arriving from it stayed in the
self type's vocabulary.

```scala
trait Q[A] { def q: A }
trait P[A] { self: Q[A] => def p: A = q }   // type mismatch; found: A  required: A
```

`Q`'s `A` and `P`'s `A` print the same and are different. In `walk`'s class arm,
the self type (instantiated with the class's type arguments) is now walked after
the parents (`crates/typer/src/symbol.rs`). Not one of slick's 54 errors moves;
the fifth case in `rej_ok.scala` is this.

The fixtures are `tests/fixtures/rej_ok.scala` (five accepting cases, dual-run,
expected output `expected/rej_ok.txt`) and `tests/fixtures/rej_bad.scala` (six
rejecting cases), with tests in `crates/cli/tests/reject.rs`. `rej_bad.scala`
cannot be made to produce everything from real scalac in one run —
`illegal inheritance` is typer, variance checking is refchecks, and nsc does not
proceed to refchecks once the typer has reported errors. The four variance cases
were confirmed separately in a file containing only those four traits.

---

### `Predef.Function`, signature-path ordering, and the lub of function types (`agent/final3`)

Seven one-off errors left in slick were reduced individually and produced **five
roots**; six errors went away and one remains. **Not one diagnostic's wording
pointed at its root**, so the write-up below is ordered by root, not by
diagnostic.

slick goes `errors=17 files_with_errors=13` →
**`errors=11 files_with_errors=9`** (`tests/slick_measure.sh`; zero new errors;
files that lost errors: `lifted/Shape.scala`,
`relational/RelationalProfile.scala`, `memory/DistributedProfile.scala`,
`compiler/FixRowNumberOrdering.scala`). The fixtures are
`tests/fixtures/final3.scala` (all cases in a single file),
`final3_use.scala` + `final3_def.scala` (two files are required because **the
command-line order** is the reproduction condition), and `final3_bad.scala`.
Tests are in `crates/cli/tests/final3.rs`. On main before the fix (`d7e7767`)
four of the five fail.

**1. `Function` is a type alias in `Predef`, not the `scala.Function` object.**
`def genericFastPath(f: Function[Any, Any])` at `Shape.scala:397` gave
`Function does not take type parameters`. The brief's reading (a type lambda
`({ type L[X] = … })#L`, possibly the same root as `agent/probe12`'s remaining
item) is **wrong**; type lambdas have nothing to do with it. `scala.Predef`
declares

```scala
type Function[-A, +B] = Function1[A, B]
```

(confirmed with real scalac 2.13.16), but the symbol table had no such alias, so
it resolved to the module class (kind arity 0) of the `object Function` that
`prelude_fntuple.rs` installs as the home of `Function.untupled`.
`tree_to_type`'s `AppliedTypeTree` already had a `Function` arm (for
`java.util.function.Function`), and there, when the resolution target is
`scala/Function$`, two arguments are now re-read as a function type.
`java.util.function.Function[A, B]` resolves to an arity-2 interface and is
unaffected. Writing `Predef.Function[A, B]` explicitly is accepted before
resolution is even attempted.

**2. `missing parameter type for expanded function` at
`RelationalProfile.scala:82` is one step downstream of 1.** Because
`genericFastPath`'s parameter type is `<error>`, no expected type reaches the
pattern-matching anonymous function being passed. **Three lines reproduce both at
once**:

```scala
object A1 {
  def genericFastPath(f: Function[Any, Any]): Any = f("x")
  val r: Any = genericFastPath(x => x)
}
```

**3. A lazy completion forced during the signature path read an
"explicitly-annotated member" that had no type yet.**
`no matching overload for constructor QueryInterpreter with arguments (<notype>, Any)`
at `DistributedProfile.scala:76`. The `<notype>` is the first argument,
`val emptyHeapDB = HeapBackend.createEmptyDatabase`.
`createEmptyDatabase: AnyHeapDatabaseDef` **writes** its result type, so it is
not a lazy subject, and its type is only installed when `HeapBackend.scala` is
walked on the signature path. But `memory/DistributedProfile.scala` comes first
in command-line order. Furthermore, **a nested template's parent clause is typed
during the outer template's "signature phase"** (`type_class` goes: signatures of
all members, then bodies of all members), so
`class DistributedQueryInterpreter(...) extends QueryInterpreter(emptyHeapDB, param)`
forced `emptyHeapDB` right there and **permanently cached** it as `<notype>` in
`lazy_done`. nsc does not hit this because every symbol has a lazy completer.
`complete_lazy_sig` was changed so that **when a completion run during the
signature path could determine nothing, it is rolled back — diagnostics and all
— to pending**. By body-path time every written signature is installed, so it is
determined correctly there. Ten lines reproduce it:

```scala
class QI(db: String, param: Any)
class DP {
  val v = HB.s
  class Sub(param: Any) extends QI(v, param)   // no matching overload … (<notype>, Any)
}
object HB { def s: String = "x" }              // ← has to come after DP
```

**4. `recursive method run needs result type` was *not* a cascade of 3** (it
survived fixing 3). It is a different consumer of the same ordering problem.
`overridden_ret_type` is deliberately committed to "do not force a candidate's
signature" (a comment records the measurement that forcing it took slick from 155
errors to 307), and `def run(n: Node): Any` in
`memory/QueryInterpreter.scala` has no type installed yet, so
`override def run(n: Node) = …` could not find anything to borrow and hit its own
recursion while still awaiting inference. The fix is **to search once more,
immediately before the body is typed** (`retry_overridden_ret`), together with
changing **`complete_lazy_sig`'s re-entry check to "cyclic only while the type is
still undetermined"** (once the result type is installed, a recursive call is not
a cycle). Only both together make it go away (either one alone did not move the
numbers — the brief's "do not conclude a fix is unrelated just because the
numbers did not move" landed exactly).

**5. The lub of function types collapsed to `AnyRef`.**
`value apply is not a member of AnyRef` at `SQLiteProfile.scala:138`. That is the
element type of
`Seq((s: String) => Timestamp…, (s: String) => …String)`. `lub` has an arm for
"same class, different arguments: join the arguments", but `FunctionN` is its own
variant `Type::Function` and so never entered it, walking base types instead and
arriving at `AnyRef`. Two `Function`s of the same arity now join with `glb` on
parameters (contravariant) and `lub` on results (covariant).

**6. A wildcard type argument carries its type parameter's declared bounds.**
`no matching overload for (Node, Option[Comprehension[Option[Node]]])Node with arguments (Node, Some[Comprehension[_]])`
at `FixRowNumberOrdering.scala:19`. Since it is
`final case class Comprehension[+Fetch <: Option[Node]]`, `Comprehension[_]` is
`Comprehension[_$1] forSome { type _$1 <: Option[Node] }`. In `is_sub_type`'s
same-symbol `Class`/`Class` arm, a bare `Wildcard` on the **left** is now re-read
as a `BoundedWildcard` whose upper bound is the type parameter's `bound_hi`. The
right side is untouched (a wildcard on the right already admits anything). This
is a different place from the `(Applied, Wildcard)` that `agent/tq` fixed. It is
**purely a relaxation**, so nothing that used to compile can start failing. That
the bound is doing work is checked in `final3_bad.scala` (a `ComprB[_]` is not a
`ComprB[Some[NdB]]`; real scalac likewise says
`type mismatch; found: ComprB[_] required: ComprB[Some[NdB]]`).

**Remaining (with a minimal reproduction and a diagnosis)**

* `jdbc/SQLiteProfile.scala:183`.
  `no matching overload for (Iterable[U], JdbcActionComponent.RowsPerStatement)…
  with arguments (Iterable[U], RowsPerStatement)` — both sides are the same name
  once you strip the prefix. `JdbcActionComponent` has a **bounded abstract type
  member**

  ```scala
  type RowsPerStatement >: slick.jdbc.RowsPerStatement.One.type <: slick.jdbc.RowsPerStatement
  ```

  which `MultipleRowsPerStatementSupport` concretises with
  `override type RowsPerStatement = slick.jdbc.RowsPerStatement`. `SQLiteProfile`
  mixes that in, so under real scalac they are identical, whereas we cannot
  as-seen-from the parent's abstract type member through the derived
  concretisation. This is a different shape from the five roots here and belongs
  to the general handling of abstract type member refinement, so it was left
  alone.

**Differences from the brief's readings**

* "Start by assuming all seven have separate roots" — in fact it was seven errors
  and five roots, with `Shape.scala:397` and `RelationalProfile.scala:82` one
  step apart on the same root.
* "In `DistributedProfile.scala`, `recursive method run` may be the root and
  `:76` the cascade" — **neither the reverse nor the same: two independent
  roots**. Fixing `:76` leaves `:91`, which needed a separate fix.
* "`Shape.scala:397` may be a type lambda (the same root as `agent/probe12`'s
  remaining item)" — no. One missing type alias in `Predef`, nothing more.
* "`FixRowNumberOrdering` is in the neighbourhood of the `(Applied, Wildcard)`
  that `agent/tq` fixed" — next door, but a different arm (the argument
  comparison in `Class`/`Class`).

---

### Differential probing, round 12 (`agent/probe12`) — 10 issues that only running revealed

slick's measurements only see **as far as type checking** (`classes=0`), so
silent runtime miscompilations can only be found by differential probing. This
round rewrote the shapes slick and cats actually use into **14 small programs**,
compiled each with both real scalac 2.13.16 and scala-rs, ran them under
`java -Xverify:all`, and **compared stdout byte for byte**. **10 of the 14
disagreed**, and the roots were mutually independent.

The measurements are **`files=184 errors=65 files_with_errors=34 classes=0`** at
both the branch point (`2a9db27`) and after the fixes (the type-checking numbers
do not move — what was fixed is runtime behaviour, plus type-checking holes slick
does not step on). codegen (`crates/backend/`) was touched, so
`tests/slick_subset.sh` was run once with `SLICK_SEED_LOG`, giving
`subset_files=38 classes=204 verified=204 failed=0` (no regression).

All 14 were promoted into `tests/conform/` (`query_ast` / `group_report` /
`show_typeclass` / `byname_lazy` / `copy_unapply` / `exception_forms` /
`number_mix` / `interp_forms` / `action_monad` / `hk_typeclass` /
`mutable_loops` / `either_validate` / `mixin_profile` / `expr_interp`). The
minimal shapes the probes do not cover are collected in
`override_val_apply.scala`.

#### Broken at runtime (compiled fine)

**1. `override val` / abstract `val` were being read as fields.**

```scala
class P { val pre: String = "a"; class T { def q = pre }; def mk = new T }
class A extends P { override val pre = "b" }
abstract class Q { val pre: String; def show = pre + "!" }
class B extends Q { val pre = "c" }
println(new A().mk.q)          // scalac: b     scala-rs: a
println((new A(): P).pre)      // scalac: b     scala-rs: a
println(new B().show)          // scalac: c!    scala-rs: null!
```

scala-rs emitted a source class's `val` as a public field and read it with a
**`getfield` on the declaring class**. A subclass that writes `override val` has
its own slot, so the override is invisible, and an abstract `val` reads a slot
nobody ever writes and comes out `null`. nsc reads every non-`private` member
value **through an accessor**, and virtual dispatch lands on the class that
actually holds the value. `reads_via_accessor` in `gen.rs` decides that condition
(not `PARAM`, not `STATIC`, not `PRIVATE`, empty `jvm_name`, and the owner is **a
class being compiled in this run**). The last condition is required: the private
runtime's `Tuple2._1` is a field and has no accessor (dropping it makes
`fixtures_predef` / `fixtures_dynamic` throw `NoSuchMethodError`).

**2. Calling an enclosing class's "method" from an inner class cast `this`.**

```scala
class Outer(val tag: String) {
  def deco(s: String) = "[" + s + "]"
  class Inner(val name: String) { def q(c: String) = tag + name + deco(c) }
}
new Outer("o").make("m").q("c")
// scalac: om[c]
// scala-rs: ClassCastException: Main$Outer$Inner cannot be cast to Main$Outer
```

The enclosing **field** (`tag`) already walked `$outer`, but the bare-`Ident`
call arm of `gen_receiver` was getting by with `load_this` plus a `checkcast`.
`load_owner_instance` is now used exactly when `this` does not conform to the
owner and the `$outer` chain does reach it (when it does not, behaviour is
unchanged). The same symptom occurred for inner classes of traits and for
abstract methods.

**3. A hand-written `unapplySeq` returning `Option[Seq[A]]` was cast to `List`.**

```scala
object Words { def unapplySeq(s: String): Option[Seq[String]] =
  if (s.isEmpty) None else Some(s.split(" ").toSeq) }
"hello" match { case Words(one) => one; case _ => "" }
// scalac: hello
// scala-rs: ClassCastException: ArraySeq$ofRef cannot be cast to List
```

The cons walk begins with `checkcast scala/collection/immutable/List`. That is
only correct for `Option[List[A]]`, and it fails for the natural spelling
`Option[Seq[A]]` (`toSeq` gives an `ArraySeq$ofRef`). It cannot be decided after
erasure has crushed `Option[Seq[A]]` to a bare `Option`, so **while the type
arguments are still there** the typer records it in
`SymbolTable::seq_extractor_payload`, and the backend reads anything other than
`List` through the same `SeqFactory$UnapplySeqWrapper$` scalac uses (or
`Array$UnapplySeqWrapper$` for arrays).

**4. `xs.view.filter(p)` claimed to be a `SeqView`.**

```scala
println(List(1, 2, 3, 4).view.filter(_ > 2).map(_ * 10).toList)
// scalac: List(30, 40)
// scala-rs: ClassCastException: View$Filter cannot be cast to SeqView
```

The 2.13 declaration is
`trait SeqView[+A] extends SeqOps[A, View, View[A]] with View[A]`, so `C` is
**`View[A]`, not itself**. The overrides that appear in
`javap scala.collection.SeqView` are only `view` / `map` / `appended` /
`prepended` / `reverse` / `take` / `drop` / `takeRight` / `dropRight` /
`tapEach` / `concat` / `appendedAll` / `prependedAll` / `sorted` — `filter` is
not among them. `check.rs`'s `returns_receiver_collection` was rebuilding the
result at the receiver, so the static type became `SeqView[A]` and the resulting
`checkcast` failed on the real `scala.collection.View$Filter`.
`prelude_viewc.rs` now declares `filter` / `filterNot` / `takeWhile` /
`dropWhile` / `collect` / `flatMap` on `SeqView` as **returning `View[A]`**, and
suppresses the rebuild for exactly those names. `View.map`'s descriptor was fixed
too (it is the erasure of `IterableOps.map: CC[B]`, i.e.
`(Lscala/Function1;)Ljava/lang/Object;`; it was calling it as `View[A]` and
getting a `NoSuchMethodError`, but nobody had tripped it because no value of type
`View` could previously be created).

**5. `Array(Array(1, 2), Array(3, 4))` was creating an `Object[]`.**
`gen_java_class_of` had no `Type::Array` arm and fell through to
`java/lang/Object`, so `Array.apply` was handed a `ClassTag[Object]` and the
result's `checkcast [[I` failed. Array class literal constants are spelled as
**descriptors**, not internal names (`[I` / `[[I` / `[Ljava/lang/String;`).

**6. A `Unit` argument in a string interpolation was never evaluated.**

```scala
println(s"unit ${println("side")}")
// scalac: side \n unit ()
// scala-rs: unit ()      ← "side" is not printed
```

`gen_sb_append` saw a `Unit` value and emitted only `ldc "()"`, never the
expression itself. It now emits it as a statement via `gen_stat` and then pushes
the constant (`gen_stat` already knows how to discard whatever a call actually
pushes).

**7. Passing a by-name argument to a local `def` / local `lazy val` forced it
twice.**

```scala
def viaLocal[A](body: => A): A = { def go(): A = body; go() }
def once[A](body: => A): () => A = { lazy val v = { println("forced"); body }; () => v }
// scala-rs: ClassCastException: java.lang.Integer cannot be cast to scala.Function0
```

Lambda lifting makes the captured by-name symbol **itself** a parameter of the
lifted method (which is why it is forced correctly inside
`v$1(Function0, LazyRef)`). But the argument at the call site is an `Ident` of
that same symbol, so erasure's `erase_ident` saw `Flags::BYNAME` and appended
`.apply()` unconditionally. The callee, handed a **value**, forced it again and
crashed. Now it does not force when the tree's type is still `ByName(_)` and the
expected type is a thunk slot (either `=> T` or a zero-argument `Function`).

#### Shapes real scalac accepts that we were rejecting

**8. `for` comprehensions over `Either`.**

```scala
type V[A] = Either[List[String], A]
for { h <- req("host"); ps <- req("port"); p <- int(ps) } yield Cfg(h, p)
// scala-rs: type mismatch; found: Either[List[String], Cfg]
//           required: Either[List[String], String]
```

`prelude_either`'s `Either.flatMap` was **monomorphic**,
`(B => Either[A, B]): Either[A, B]`. nsc has
`def flatMap[A1 >: A, B1](f: B => Either[A1, B1]): Either[A1, B1]`. Because the
continuation was pushed back to the receiver's own `B`, every `for`
comprehension whose right type changes from step to step was a type error.

**9. `implicit class` with an implicit parameter clause.**

```scala
implicit class ShowOps[A](a: A)(implicit s: Show[A]) { def shown = s.show(a) }
// scala-rs: no implicit: could not find implicit value of type Show[A]
//           (the error is reported on the class declaration itself)
```

`implicit_class_conversions` only looked at `vparamss.first()` and threw away the
second and later clauses. That made `new ShowOps[A](a)` summon a `Show[A]` for an
abstract `A`. Following nsc's desugaring, the remaining clauses are carried on
the conversion method and passed straight to the `new`. Every cats-style syntax
class (`implicit class MonadOps[F[_], A](fa: F[A])(implicit m: Monad[F])`) was
failing on this.

**10. `f(x)()` — applying a `() => A` returned by a method on the spot.**

```scala
def mk(n: Int): () => Int = () => n
println(mk(3)())   // scala-rs: not enough arguments: expected 1, found 0
```

The empty argument clause was being read as `mk`'s second parameter clause. If
the type of an already-applied `Apply` is a `Function`, that clause is
`Function0.apply`, not a parameter clause of the callee (the same test as
erasure's `sym_denotes_callee`).

#### Differences not fixed (input for the next slice)

* **`xs.flatten`.**

  ```scala
  val opts: List[Option[Int]] = List(Some(1), None, Some(3))
  println(opts.flatten)      // scalac: List(1, 3)
  // scala-rs: value sum is not a member of ((Option[Int]) => IterableOnce[B])List[B]
  ```

  The implicit clause of the pickle's
  `IterableOps.flatten[B](implicit toIterableOnce: A => IterableOnce[B]): CC[B]`
  **stays unapplied**, and the method type becomes the result (the type shown is
  that raw method type). Real scalac passes `Predef.$conforms`
  (`invokevirtual List.flatten:(Lscala/Function1;)Ljava/lang/Object;`). Filling
  it requires **implicit search that solves a type variable backwards from the
  result type**: "conform `<:<[A, A]` to `A => IterableOnce[B]` while solving
  `B`". `List[List[Int]]` is the same.

* **The type lambda `({ type L[X] = Reader[R, X] })#L`.** The shape cats uses
  without kind-projector.

  ```scala
  implicit def readerMonad[R]: Monad[({ type L[X] = Reader[R, X] })#L] = …
  // scala-rs: type mismatch; found: $anon$1  required: Functor[<none>.L]
  //           type mismatch; found: Any  required: R
  ```

  A projection onto a type member inside a refinement cannot be resolved as a
  type constructor and becomes `<none>.L`. Assignment to `Functor[IntReader]` via
  the type alias `type IntReader[X] = Reader[Int, X]` does not work either.

* **A method named `def using(...)`** (a difference in what is accepted). Real
  scalac 2.13.16 **rejects** `using(r)(f)` with
  `Main.Res does not take parameters` (`using` is a soft keyword for argument
  lists, so it reads as `(using r)(f)`). scala-rs accepts it as an ordinary
  identifier. Not a miscompilation — scala-rs is simply more permissive. The name
  was changed to `withRes` in `tests/conform/exception_forms.scala`.

#### Tests run

`cargo test --workspace --release` (after the whole fix set, before adding the
conform files) is green. Then `--test conform` alone gives **77 passed** (62
before plus 15 new). `--test e2e` is 460 passed. `cargo fmt --all` done;
`cargo clippy` reports zero new warnings.
