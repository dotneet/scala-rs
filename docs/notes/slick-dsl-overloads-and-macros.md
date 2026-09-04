# Overload resolution, the slick DSL, and macro expansion

Development notes for the slices that chewed through slick's own API surface:
`DBIOAction` / `JdbcActionComponent`, the `no matching overload` /
`ambiguous overload` cluster, `TableQuery` / `Compiled`, and the `ShapedValue`
macro. Overload resolution is where the typer, the pickle reader, and the
classfile reader all meet, so almost none of these diagnostics pointed at their
own root — and a recurring observation across the slices is that neither "the
same symptom" nor "the same file" implies a single root.

---

### The five roots behind 13 errors in slick's `JdbcActionComponent` / `DBIOAction` (`agent/dbio`)

Thirteen errors across two files came from five roots, every one of them
**upstream of the symptom**, with a single root emitting three errors at a time:
named arguments to a parent constructor, `private[this]` not being inherited,
two erased prelude signatures, a lower bound that was thrown away, and typed
patterns losing the scrutinee's type arguments.

`tests/slick_measure.sh` goes **`errors=99 → 90`, `files_with_errors=39 → 39`**
(all nine errors that disappeared are in these two files; zero new errors). The
two files I owned went **from 13 errors to 4** (`JdbcActionComponent.scala`
7 → 1, `DBIOAction.scala` 6 → 3). codegen was touched, so
`tests/slick_subset.sh` was run too:
`subset_files=38 classes=204 verified=204 failed=0`.

**1. Named arguments to a parent constructor** (3 errors). As described in the
section "implicit / default arguments to a parent constructor". A single site,
`extends SimpleJdbcProfileAction[R](_name = …, statements = …)`, produced three
errors: `not found: value _name`, `not found: value statements`, and
`no matching overload for constructor … with arguments (Unit, Unit)`.

**2. `private[this]` members are not inherited** (one error on its own, two more
together with root 5). Per SLS 5.2, `private[this]` belongs to **that
instance**, so the prefix of an unqualified reference can only be "the `this` of
my own class". slick writes:

```scala
trait SynchronousDatabaseAction[+R, +S, C, -E] extends DatabaseAction[R, S, E] { self =>
  private[this] def superZip[R2, E2 <: Effect](a: DBIOAction[R2, NoStream, E2]) = super.zip(a)
  override def zip[R2, E2 <: Effect](a: DBIOAction[R2, NoStream, E2]) = a match {
    case a: SynchronousDatabaseAction[?, ?, ?, ?] => new SynchronousDatabaseAction.Fused[(R, R2), NoStream, C, E with E2] {
      override def nonFusedEquivalentAction: DBIOAction[(R, R2), NoStream, E with E2] = superZip(a)
    }
```

The anonymous class is a `SynchronousDatabaseAction` at **different type
arguments** (`R = (R, R2)`), so reading `superZip` "through this class" gave
`DBIOAction[((R, R2), R2), NoStream, E with E2 with E2]` (and `superAsTry` gave
`Try[Try[R]]`). `enter_inherited_members` does **not** put `private[this]` into
the child's scope, so name resolution was hitting the outer one from the start;
the only thing wrong was the `subst_as_seen_from` in `bind_found`. Writing
`superZip` as public instead of private makes **real scalac produce the same
mismatch we did**, so this is a shape specific to `private[this]`
(`tests/fixtures/db.scala`).

This has two consequences on the codegen side as well.

- The call receiver has to walk outward **by identity** too (`gen_ident`'s
  `is_private_this` → `load_self_alias_instance`). `this` conforms to the owner,
  so `load_owner_instance` stopped right there and read the anonymous class's
  own `r` (the same trap as the self-type alias in `agent/tail3`).
- Since it arrives from another class, the JVM sees a cross-class call to
  `ACC_PRIVATE`, i.e. `IllegalAccessError`. The same `access_widened` used for
  reads through a companion is raised.

**3. `Either.getOrElse` / `Try.getOrElse` were `(=> Any): Any`** (3 errors,
together with root 4). The prelude's `add_either` / `add_try` signatures were
**erasing the result** rather than widening it. slick's

```scala
val prit = inv.results(0, …)(ctx.session).getOrElse(throw new NoSuchElementException)
val rows = prit.map(value => new Mutator(value, prit.pr, inv))
```

emitted `… is not a member of Any` at every use, so one signature produced three
errors. nsc has `getOrElse[B1 >: B](or: => B1): B1` /
`getOrElse[U >: T](default: => U): U` (`crates/typer/src/prelude_dbio.rs`; the
same shape `prelude_ovl3` applied to `Option.getOrElse`). Erasure is unchanged
(an unbounded type parameter erases to `Object`), but **the call site now needs
a checkcast**, so the hand-written `getOrElse` path for `Either`/`Try` in gen.rs
goes through `lazy_cell_from_object` and not just the primitive unbox.

**4. A `[B >: A]` lower bound was discarded when it mentioned a caller's type
parameter** (the other half of the same three errors as root 3; fixing only 3
merely turned `is not a member of Any` into `is not a member of Nothing`).
`tparam_lower_bound` discarded the lower bound, once read through the receiver,
**if it mentioned any type parameter at all**. The only ones it is safe to
discard are the **owner's own** (i.e. those not readable through the receiver)
and **the method's own** (i.e. the variables this call is trying to solve).
Type parameters of the enclosing method are fixed types here, so

```scala
def use[T](e: Either[Int, It[T]]) = e.getOrElse(throw new NoSuchElementException).xs
```

solved `B1` to the argument's `Nothing` and gave
`value xs is not a member of Nothing` (it shows up as "writing `It[String]`
makes it work").

**5. A typed pattern keeps the scrutinee's type arguments** (2 errors). This is
nsc's `inferTypedPattern`.

```scala
case a: SynchronousDatabaseAction[?, ?, ?, ?] => … superZip(a) …
```

Binding `a` as a bare `SynchronousDatabaseAction[_, _, _, _]` throws away the
`R2` / `NoStream` / `E2` that the scrutinee already stated, so it cannot be
passed to `superZip(a: DBIOAction[R2, NoStream, E2])`. The parameters are solved
from the **base type of the pattern's class in the scrutinee's class**, filling
in only the positions written `_` (`pattern_targs_from_scrutinee`). The result
stays a plain class type rather than an intersection, so erasure and codegen are
unchanged. Parameters the scrutinee does not determine (slick's `C`, which
`DBIOAction` does not take) stay `_`.

#### Tests

Seven tests in `crates/cli/tests/dbio.rs`, with three fixtures prefixed `db`.
**Six of the seven fail on main before the fix** (the remaining one is a
negative test checking that `Either` is diagnosed under `--no-scala-library`,
which passes on main too).

* `tests/fixtures/db.scala` (+ `expected/`) — roots 1, 2, 4 and 5 in one file.
  It uses no standard library, so it runs in **both modes** and dual-runs
  against real scalac.
* `tests/fixtures/db_lib.scala` (+ `expected/`) — root 3. `Either` / `Try` are
  library-ABI only (`prelude::add_either` lives inside `library_abi`), so jar
  mode only. That `--no-scala-library` diagnoses it is pinned as well.
* `tests/fixtures/db_bad.scala` (the rejecting side) — a parent constructor with
  a misspelled named argument. It must produce the same `unknown parameter name:
  stmt` as real scalac. The reason the tree is not rewritten when reordering
  fails is that consuming named arguments on the signature path (which discards
  diagnostics) would leave the body path with nothing but
  `no matching overload`.

#### Remaining

The four errors left in the two files each have their own root, and in every
case the minimal reproduction was checked to be **accepted by real scalac
2.13.16**.

* **Inferring type arguments when passing `<:<` as a `Function1`**
  (`DBIOAction.scala:52`,
  `def flatten[R2, S2, E2](implicit ev: R <:< DBIOAction[R2, S2, E2]) = flatMap(ev)`).
  Conformance itself works — writing `val g: R => Act[R2] = ev; flatMap(g)`
  compiles. What fails is **solving `R2` from the argument** in
  `flatMap[R2](f: R => Act[R2])`: the argument is the *class* `<:<[R, Act[R2]]`,
  and its base type at `Function1` is not read before matching against a
  `Type::Function`'s parameters.
* **Overloads taking a fixed type parameter as an argument**
  (`DBIOAction.scala:367`, the `value: R` of `String.valueOf(value)`).
  `arg_score` has an arm
  `if matches!(param, TypeParam(_)) || matches!(arg, TypeParam(_)) { Some(2) }`,
  so **if the argument's type is a type parameter, every candidate matches**.
  Minimal reproduction:

  ```scala
  object Q { def h(x: Any) = "any"; def h(x: Boolean) = "bool"; def h(x: Long) = "long" }
  def c[R](v: R) = Q.h(v)          // ambiguous overload for h with arguments (R)
  object O { def f(x: Any) = "any"; def f(x: Int) = "int" }
  def a[R](v: R) = O.f(v)          // type mismatch; found: R  required: Int
  ```

  `R` is not a variable being solved but a **fixed type**, so conformance has to
  be `is_sub_type(R, param)` (i.e. only the `Any` arms above). Dropping the
  `arg`-side arm is the fix, but `arg_score` is the place every overload
  resolution goes through, so it was left alone in this slice.
* **A parameterless polymorphic method in argument position**
  (`JdbcActionComponent.scala:725`,
  `session.withPreparedInsertStatement(sql, keyColumns.toArray)(f)`).
  `ConstArray`'s `def toArray[R >: T : ClassTag]: Array[R]` does not go through
  `instantiate_parameterless` without an expected type (exactly as the comment
  "only when there is an expected type" says), so it stays `Array[R]` and
  matches both `(String, Array[String])` and `(String, Array[Int])`, giving
  ambiguity. nsc solves `R` to its lower bound `T` first and then resolves.
* **`cats.effect.IO(fa)`** (`DBIOAction.scala:237`). It shows up as "`IO$`'s
  `apply` is not found", but it **does not reproduce in isolation** — writing
  the same expression as
  `LiftF[cats.effect.IO, R](cats.effect.IO.fromFuture(cats.effect.IO(fa)))`
  (with the sibling `from[F[_], R]` overload included) compiles. It only appears
  when the whole of slick is read at once, so an ordering dependence in member
  supply from pickles is suspected.

`no matching overload for (Iterable[U], RowsPerStatement)…` at
`SQLiteProfile.scala:183` was investigated on the assumption that it was a
cascade of root 1, but it survives the named-argument fix (a different root).

---

### Nine of slick's 26 overload-resolution errors, six roots (`agent/ovl4`)

Each of the 26 `no matching overload` / `ambiguous overload` errors was reduced
to a minimal reproduction one at a time. The six roots that came out were
mutually unrelated, and three pairs of errors **in different files shared a
root** — the mirror image of the usual assumption.

`tests/slick_measure.sh` goes **`errors=65 → 55`, `files_with_errors=34 → 31`**
(zero new errors). The cluster I owned — 21 `no matching overload` and 5
`ambiguous overload` — went **from 26 to 17**. As a bonus,
`value infer is not a member of AnyRef` (`Comprehension.scala:85`) went away
with the sixth root. Only the type checker was touched, so
`tests/slick_subset.sh` was skipped (`crates/backend/` unchanged).

Reducing all 26 to minimal reproductions confirmed the existing observation that
**neither "the same symptom is one root" nor "the same file is one root" holds**.
None of the six roots are related to each other, and conversely there were three
pairs where **two errors in different files shared a root**.

**1. The "argument" of a fixed type parameter is only what its upper bound
gives.** `arg_score` had an arm

```rust
if matches!(param, Type::TypeParam(_)) || matches!(arg, Type::TypeParam(_)) {
    return Some(2);
}
```

so **a bare type parameter as the argument's type matched every candidate**.
`String.valueOf(value)` (`DBIOAction.scala:367`, with
`case class SuccessAction[+R](value: R)`) matched everything from
`valueOf(Object)` down to `valueOf(char)`, giving `ambiguous overload`. nsc
picks `valueOf(Object)` (confirmed with `javap -c`; `Any` conforms to a Java
`Object` parameter — 2.13's `ObjectTpeJava`).

The `param`-side arm is correct (the `T` of `def f[T](x: T)` is the candidate's
own variable and is not in `undet_tvars` during scoring), so it was kept; the
`arg` side was replaced with **retrying at the upper bound**. `is_sub_type`
already widens to the upper bound, so this arm only bites when "the parameter
mentions the candidate's own unsolved variables" — which is
`Comprehension.scala:22`, passing the `fetch: Fetch` of
`Comprehension[+Fetch <: Option[Node]]` to `ConstArrayBuilder.++`'s three
overloads (`ConstArray[T]` / `IterableOnce[T]` / `Option[T]`), where only
looking at the upper bound `Option[Node]` leaves `Option[T]` standing.

The brief said "the fix is to drop this arm", but **dropping it alone produces
two new errors in `Comprehension.scala`**. Retrying at the upper bound is part
of the same change.

**2. Compound types (`A with B`) as parameters and arguments**
(`JdbcTypesComponent.scala:50`, `MemoryProfile.scala:62`,
`MemoryProfile.scala:63`). slick writes

```scala
type BaseColumnType[T] = ScalaType[T] with BaseTypedType[T]
def assertNonNullType[A](t: BaseColumnType[A]): Unit
```

and calls `assertNonNullType(implicitly[BaseColumnType[U]])`. Neither
`class_ctor_matches_typeparam_args` ("matches if the parameter's type arguments
are type parameters") nor `unify_one` looked at `Type::Refined`, so nothing
matched, and even forcing a match left `A` unsolved. Two rules were added to
both:

* compound against compound matches **component by component**, and
* an argument that is compound matches if **any one component** matches (passing
  `ScalaType[U] with BaseTypedType[U]` to the parameter of
  `ColumnType[U'] = ScalaType[U']` in `new MappedColumnType(...)`).

**3. A monomorphic callee also passes parameter types down as expected types.**
`proto_arg_type` only emitted prototypes for "function-shaped parameters" when
the callee had no type parameters. nsc types **every** argument against its
parameter type. The difference shows up exactly when **the argument's own type
parameters are decided by inference** — `RefId[E <: AnyRef]` is invariant, so

```scala
val errors = mutable.Set.empty[RefId[Dumpable]]
errors += RefId(n1)            // n1: Node
```

only gets `E = Dumpable` because there is an expected type `RefId[Dumpable]`
(`VerifyTypes.scala:38,41`). Prototypes remain **hints** under the existing
caller-side discipline: if the argument does not conform, it is retyped with no
expected type. There were no regressions across the 460 e2e tests or the seam
list.

**4. A fixed type parameter goes through its upper bound during inference too.**
After root 1's conformance succeeded, passing `fetch: Fetch` to
`mapOrNone[A](o: Option[A])(f: A => A)` left `A` unsolved, so it fell to `Any`
and `_.infer(scope, …)` gave `value infer is not a member of Any`
(`Comprehension.scala:85`; a separate error outside the 26). `unify_one` is a
free function with no symbol table, so the retry — "if nothing could be
inferred, retry at the upper bound" — was put on the `unify_tparam_all` side.

**5. Constructors are not inherited.** Even when handed a `Type::Overload`,
`resolve_overload` **rebuilds** the candidate table via
`overload_alternatives` (which ultimately calls `lookup_member`).
`lookup_member` walks parents, so the candidates for
`java.util.Properties`'s `<init>` picked up `Hashtable`'s `(Int, Float)` and
`(Map[_ <: K, _ <: V])`, and `new Properties(null)` became `ambiguous overload`
between `Properties(Properties)` and `Hashtable(Map)`
(`GlobalConfig.scala:68`). `pick_ctor_at` filtered on `owner == class_id`, but
this path was dropping that.

However, **filtering to "owner matches exactly" breaks things**: the same
classfile can enter the symbol table by two routes, and
`java.io.OutputStreamWriter` is exactly that case — only one of the copies of
`OutputStream` was a parent of `PrintStream`. What gets dropped is limited to
**those whose owner is a proper superclass** (`owner_is_proper_subclass`).

**6. A `-cp` stub is a subtype of nothing.** Root 5's filtering broke
`new OutputStreamWriter(System.out)`, and investigating showed that
`Writer(Object lock)` — an inherited constructor that nsc would not even
consider a candidate — had merely been making it look successful. The real
reason is that the stub `find_or_stub_java_class` builds from a descriptor has
only `parents = [AnyRef]`, so before anyone has read `java/io/PrintStream`'s
classfile it does not conform to `OutputStream`. **The same expression works
later in the same file** (because some other path read it first). `arg_score`
takes `&self` and so cannot read classfiles — the same shape as the
`Option.option2Iterable` case — so a "if it fails once, read the argument's
class and try again" step (`warm_java_args`) was added on the `new` side too.

#### What these six removed

Ten errors: `Comprehension.scala:22,85`, `ExpandSums.scala:27`,
`VerifyTypes.scala:38,41`, `DBIOAction.scala:367`,
`JdbcTypesComponent.scala:50`, `MemoryProfile.scala:62,63`,
`GlobalConfig.scala:68`. `ExpandSums.scala:27`
(`oldDiscCandidates ++ (tree match { … })`) had been read as caused by a
`Set[_ <: AnyRef]` lub, but it actually went away with root 3 — one more
instance of **a symptom-based reading being unreliable**.

#### Tests

`crates/cli/tests/ovl4.rs` (five tests) and the fixtures
`tests/fixtures/ovl4.scala` / `ovl4_bad.scala`. The six roots are **collected
into one file** (a single real-scalac run costs 1.8 s, so the fixture is made
wider rather than more numerous). On main before the fix `ovl4.scala` produces
**seven errors in both modes**. Dual-run output was confirmed identical across
all three of real scalac 2.13.16, `--scala-library`, and `--no-scala-library`.
`ovl4_bad.scala` is the flip side of root 1 —
`def bad[T](x: T) = takesList(x)` — which real scalac also rejects with
`type mismatch; found: T required: List[Int]`.

What was run: `--test ovl4 --test overloadshadow --test ambigmap --test setapply
--test uniteq --test integral --test ordsummon --test mutcoll --test conform
--test ovl2 --test ovl3 --test mismatch14 --test seqfn --test arrconv
--test buildfrom --test dbio --test e2e` (all green).

#### A known diagnostic regression (one case)

Before root 1, passing a bare type parameter to a call with a single candidate
would match and then have `adapt` produce
`type mismatch; found: T required: List[Int]` (the same wording as nsc). Now it
fails at the conformance stage, so it says
`no matching overload for (List[Int])Int with arguments (T)`. This is the known
coarseness `agent/ovl3` wrote down — **`no matching overload` is emitted even
for a single candidate** — and the fix is "if there is exactly one candidate,
`adapt` the argument and report the real mismatch", but that touches the
expected strings of many existing tests, so it was not done in this slice.

#### The remaining 17 (minimal reproductions and readings)

* **`Array` is not seen as one of the `Seq` family** (`TupleSupport.buildTuple(a)`
  at `ResultConverter.scala:58`, `Map(...) ++ anArrayOfTuples` at
  `JdbcTypesComponent.scala:526`). Not just
  `def f(x: Seq[Any]) = 1; f(a: Array[Any])` — even
  `def v(a: Array[Any]): Seq[Any] = a` fails (real scalac accepts it). The
  prelude has only `wrapIntArray` and `wrapBooleanArray`, and moreover
  `seqfn_view.rs::array_seq_wrap` only answers for `Boolean`. The fix is to add
  `wrapRefArray[T](Array[T]): ArraySeq$ofRef[T]`, branch `array_seq_wrap` on the
  element type, and consult it from both `adapt` and `arg_score`.
  `genericWrapArray` cannot be used — the real ABI's descriptor is
  `(Ljava/lang/Object;)…` while our backend erases `Array[T]` to
  `[Ljava/lang/Object;`.
* **`Set() ++ xs`** (`JdbcModelBuilder.scala:280`). `Set()` freezes into
  `Set[Nothing]`, and the only `++` candidate is `(IterableOnce[A])Set[A]` with
  `A = Nothing`, so nothing can be passed (`Set() ++ List("a")` reproduces it
  too). nsc has two: `SetOps.concat(IterableOnce[A])` and
  `IterableOps.concat[B >: A]`, and solves `B` with the latter. This is a
  prelude/pickle seam, so tread carefully.
* **`ConstArray.toArray`** (`JdbcActionComponent.scala:725`). Exactly as the
  brief read it. `def toArray[R >: T : ClassTag]: Array[R]` stays `Array[R]`
  with no expected type and matches both `(String, Array[String])` and
  `(String, Array[Int])`. The minimal reproduction is
  `s.withPreparedInsertStatement(sql, ks.toArray)(f)` verbatim. nsc treats `R`
  as an undetermined variable of the whole call and solves it at the lower bound
  `T = String` — that is about putting the argument's undetermined variables
  into `undet_tvars`, and dropping root 1's arm would not change it (the
  argument's type is `Array[R]`, not a bare `R`).
* **`FixRowNumberOrdering.scala:19` / `ExpandSums.scala:245`**. `fix(ch, Some(c))`
  (where `c` is the existential bound by `case (c: Comprehension[?], _)`) and
  `ProductNode(ConstArray(disc, map)).infer()`. Naively rewritten minimal
  reproductions were **rejected by real scalac too**, so the variance of the
  skolem bound by the pattern is doing real work here. Unexplained.
* **Three cascades**: `Node.scala:534` (no `:@` extractor),
  `CreateAggregates.scala:100` (`.toMap`'s implicit argument is not inserted, so
  the result stays a method type), `ExpandTables.scala:25` (`collection.Map` has
  no `contains` / `apply`). In all three the root is a different diagnostic one
  line above in the same file, and none of them is an overload problem.
* The rest (`QueryCompiler.scala:220`, `SQLiteProfile.scala:183`,
  `JdbcModelBuilder.scala:93,159`, `DistributedProfile.scala:76`,
  `DBIOAction.scala:52,237`) did not reproduce under naive shrinking.
  `agent/dbio`'s observation that `cats.effect.IO(fa)` at
  `DBIOAction.scala:237` only appears with the whole of slick still stands.

---

### slick's `TableQuery` / `Compiled`: five errors, three roots (`agent/tq`)

Five errors left in `lifted/TableQuery.scala`, `lifted/Compiled.scala` and
`relational/RelationalProfile.scala`. **Five errors, three roots**, and in none
of them was the place the diagnostic named the actual root — one was an arm
ordering in `is_sub_type`, one was `TypeApply` typing its callee in value
position, and one was an implicit candidate whose own type parameters could not
be solved.

Every case was checked to compile under real scalac 2.13.16
(`/tmp/scala-2.13.16/bin/scalac`) as a minimal reproduction before being fixed.
Tests are in `crates/cli/tests/tq.rs`; fixtures are `tests/fixtures/tq.scala`
(all cases in one file) and `tests/fixtures/tq_bad.scala`.

slick: `errors=44 files_with_errors=26` → **`errors=38 files_with_errors=25`**
(the five I owned plus one fixed as collateral; no new errors;
`tests/slick_measure.sh`).

**1. An applied abstract type constructor does not fit under a wildcard** (the
diagnostic says "bounds violation").

```scala
trait Rep[T]
trait QueryBase[T] extends Rep[T]
trait Query[+E, U, C[_]] extends QueryBase[C[U]]

def t6[BU, C[_]](x: Rep[C[BU]]): Rep[_] = x   // ← this is what fails
```

The diagnostic is

```
type arguments [Query[B, BU, C],C[BU],BU] do not conform to method apply's
type parameter bounds [T <: Rep[_],TU,EU]
```

which looks like the **bounds check** of
`StreamingExecutable.apply[T <: Rep[_], TU, EU]`. In fact the bounds check works
correctly; the failure is where `Rep[C[BU]]` — reached by walking `Query[B, BU, C]`
up to its parent — is compared with `Rep[_]`. `Rep` is invariant, so it reduces
to comparing the arguments, `C[BU] <: _`, and that was returning false. If `C`
is a **concrete** type constructor such as `Seq` it works (`t4` passes), which
is why the symptom looks like "higher-kinded bounds".

The root is the **arm order** in `is_sub_type`. The
`(Type::Applied { ctor, args }, other)` arm catches everything without looking
at the right-hand side, and it sits **before** the `Type::Wildcard` arm. Inside
it, `bound_hi` is only followed when `ctor` is a `TypeMember`; for a type
**parameter** (`C[_]`) it returned `false`. Fixed by placing
`(Applied, Wildcard)` and `(Applied, BoundedWildcard)` ahead of the `Applied`
arm (`crates/typer/src/symbol.rs`).

**2. `TypeApply`'s callee was typed in value position.** Macros are irrelevant.

```scala
class TQ[E](cons: Int => E)
object TQ {
  def apply[E](cons: Int => E): TQ[E] = new TQ[E](cons)
  def apply[E]: TQ[E] = null            // ← the argument-less one
}
TQ.apply[String](f)   // error: value apply is not a member of TQ[String]
```

The brief guessed "slick defines `TableQuery.apply[E]` as a **macro**, so this
may be a problem with resolving macro definitions", but **macros have nothing to
do with it**: the shape above reproduces with no macros at all. The earlier
diagnosis `§7.13 (overload resolution)` was closer; precisely, it is **when the
overload set gets collapsed**.

By SLS 6.26.3, an overloaded reference in value position keeps only the
**candidates that take no arguments**. `Apply` types its callee with a
`Type::Method` expected type to stop that collapse, but `TypeApply` was typing
its own `fun` with `Type::NoType` (i.e. value position). `TableQuery.apply[E]`
is a `TypeApply`, so it collapsed to the argument-less alternative before the
enclosing `Apply` ever saw the arguments, which left an attempt to apply `(cons)`
to a `TableQuery[E]` and hence "value apply is not a member of TableQuery[E]".
Explicit type arguments do not narrow it either (both candidates take one type
argument). nsc types `typedTypeApply`'s `fun` in FUNmode and so does not
collapse.

The fix passes the expected type down to `fun` only when the `TypeApply` is
itself the callee of an `Apply` (i.e. `pt` is `Type::Method`). If the set
survives, `Apply`'s existing `pending_targs` applies the explicit type
arguments.

A `Type::Method` expected type, however, **also suppresses auto-application of
parameterless methods**. fs2's `Stream.fromIterator[F]` is a parameterless
polymorphic method whose result has an `apply` that takes arguments (a
partially-applied builder). Passing the expected type down naively made
`fromIterator[IO](it, chunkSize = 1)` an application to a parameterless method,
**growing one new error** in `slick/cats/Database.scala`. Since the expected type
exists solely for the sake of the overload set, auto-application is re-applied
exactly as in value position whenever the result is not an `Overload`
(`crates/typer/src/check.rs`).

**3. When the output type is undetermined at the call site, an implicit
candidate's *own* type parameters cannot be solved.**

```scala
def apply[V, C <: Compiled[V]](raw: V)(implicit compilable: Compilable[V, C], …): C
implicit def function1IsCompilable[A, B <: Rep[_], P, U](implicit
  aShape: Shape[ColumnsShapeLevel, A, P, A],
  pShape: Shape[ColumnsShapeLevel, P, P, _],
  bExe: Executable[B, U]): Compilable[A => B, CompiledFunction[A => B, A, P, B, U]]
```

The `C` of `Compiled { (p: Rep[P]) => … }` is not determined by the arguments
(it appears only in the result type and the implicit clause). The existing
`undet_solution` gets as far as searching for
`Compilable[Rep[P] => Query[T, U, Seq], ?C]` with `C` still open. Unifying with
the candidate's result type determines `A` and `B` and binds
`?C := CompiledFunction[A => B, A, P, B, U]`, but the candidate's own `P` and
`U` have **no counterpart on the wanted-type side** and stay undetermined.
`implicit_solve` demands a complete solution from the result type alone, so it
dropped the candidate and slick's own `@implicitNotFound` came out as

```
Computation of type (Rep[P]) => Query[T, U, Seq] cannot be compiled (as type C)
```

**This is not a scala-rs message** (exactly as the brief noted). The
`type mismatch; found: C required: CompiledFunction[…]` is the aftermath; the
two errors are one root.

The only place that can speak for `P` and `U` is the candidate's **own** implicit
clause (`aShape: Shape[…, A, P, A]` gives `P`, `bExe: Executable[B, U]` gives
`U`). nsc puts these into `Context.undetparams` while typing the implicit
arguments and solves them there. `implicit_fit_open`
(`crates/typer/src/implicits.rs`) was added: only for candidates that failed
normal resolution, the leftover type parameters of the candidate are solved from
its own implicit clause as the undetermined set of `search_implicit_undet`. It
is conditioned on being a **fallback**, and on **the wanted type determining at
least one of the candidate's type parameters** (a candidate with everything
undetermined would match every implicit in scope).

**Fixed as collateral**: `value apply is not a member of
SqlStreamingAction[Vector[Unit], Unit, Effect]` (same as root 2).

**What I verified**: `--test tq conform buildfrom buildfrom2 asttype hkinfer
overloadshadow ambigmap setapply uniteq integral ordsummon mutcoll ovl2 ovl3 ovl4`
plus `cargo test --workspace --release`. `crates/backend/` was untouched, so
`tests/slick_subset.sh` was skipped.

**Remaining**:

* Root 1 only fixes the `Rep[C[BU]] <: Rep[_]` direction. Other matchings with
  `C[BU]` on the **left** (things like `C[BU] <: Iterable[_]` that need to follow
  the constructor's bound) still return `false` in the `Type::Applied` arm. They
  did not appear in slick.
* Completing root 3 only runs the candidate's implicit clauses once each in
  **written order**; it does not handle the shape where a later clause narrows an
  earlier clause's solution (mutually recursive resolution).
* What remains in those same three files is unrelated.
  `TableQuery.scala:16`'s `cons(new BaseTag { base => … })` (the anonymous
  class's **self name** `base` is not visible from the body),
  `RelationalProfile.scala:72:71`'s
  `could not find implicit value of type TypedType[Boolean]`, and
  `82:61`'s `missing parameter type for expanded function`. Root 3's two errors
  were on **the same line** 72, but were unrelated to the
  `TypedType[Boolean]` at 72:71 and disappeared without fixing that.

---

### `ShapedValue.mapToImpl` — `MemberScope#collect`, a refined `Context`, and mixed `..$` (`agent/shaped`)

Five errors in slick's `lifted/ShapedValue.scala` were reduced to zero. Three
roots explained them; once the third was fixed, the remaining two (quasiquote
diagnostics about `<error>`-typed holes) turned out to be **cascades** of the
earlier ones and disappeared together.

See [`docs/macros.md`](docs/macros.md) §7.16. `tests/slick_measure.sh` goes
**`errors=99 → 94`, `files_with_errors=39 → 38`** (zero new errors). codegen
(`crates/backend/`) was untouched, so `tests/slick_subset.sh` was skipped.

#### 1. `MemberScope` cannot be read as an `Iterable[Symbol]` (`crates/typer/src/pickle_supply.rs`)

`rTag.tpe.decls.collect { case s: TermSymbol => … }` — the first line of
`mapToImpl` — gave `value collect is not a member of Scopes.MemberScope`. The
real scala-reflect hierarchy is

```text
type MemberScope >: Null <: AnyRef with Scope with MemberScopeApi
trait MemberScopeApi extends ScopeApi
trait ScopeApi extends Iterable[Symbol]
```

and neither `MemberScopeApi` nor `ScopeApi` **has a pickle of its own**
(`javap`'s `Scopes$MemberScopeApi` shows `interfaces: 0`). `PickleSupply::complete`
had been changed to "if the member is not found, also ask **library ancestors**",
but that ancestor list was **a snapshot of the parent list at call time**. A
stub's parent list is empty until its pickle is read, so **a climb of two or
more levels stops at the first**: it reaches `MemberScopeApi`'s pickled parent
`ScopeApi`, and even though `complete_on(ScopeApi)` attaches `Iterable[Symbol]`
immediately afterwards, **nobody ever asks `Iterable`**.

It was replaced with `complete_on_ancestors`, which goes **one level at a time,
reading that level's pickled parents before moving on**. The ordering (parents
from the back, breadth first) is unchanged, so which ancestor answers is
unchanged. The only thing that changed is that it now reaches below.

#### 2. Members read through an abstract type member were **not substituted** (`crates/typer/src/symbol.rs`)

With 1 fixed, `collect` is found, but `decls.toList` returned `List[A]` — still
`Iterable`'s own type parameter. `SymbolTable::subst_as_seen_from`'s `walk` had
no arms for `Type::TypeMember` / `Type::TypeParam` and fell through to
`_ => ty`. **A member read from an abstract type member is declared by that
member's upper bound**, so the substitution now follows the upper bound. This
makes `decls`' element type genuinely `Symbol`, so `s.isVal` / `s.isCaseAccessor`
/ `s.typeSignature` compile.

#### 3. `blackbox.Context { type PrefixType = … }` (`crates/typer/src/macros.rs`)

The definition of `mapTo` gave

```
error: macro implementation ShapedValue.mapToImpl must take
       scala.reflect.macros.blackbox.Context (or the whitebox one) as its first parameter
```

slick's implementation is
`c: blackbox.Context { type PrefixType = ShapedValue[?, U] }` — nsc's own idiom
for giving `c.prefix` a type — and `macro_context_kind` only looked at
`Type::Class`. The **parents of a refinement** are now candidates as well, and
as a **last resort** so is the **erased descriptor of the first parameter**. The
latter is needed when reading back from scala-rs's own classfiles (our pickle
drops the refinement, and it reads back as `Any`). If the first parameter really
is `Any`, its descriptor is `java.lang.Object`, which is neither `Context`, so we
**refuse as before**.

#### 4. `..$xs` mixed with ordinary elements (`crates/typer/src/reify.rs`)

`q"f(a, ..$xs, b)"` gave "`..$` splice mixed with ordinary arguments is not
reified yet". It now matches nsc's `reifyList`: **consecutive ordinary elements
are collected into one `List(...)`, rank-1 holes are left as they are, and the
pieces are joined left to right with `++`** — `List(<a>) ++ xs ++ List(<b>)`.
Argument order is concatenation order and every fragment is already a
`List[Tree]`, so there is nowhere to guess a static type. It works in argument
clauses, pattern argument clauses, block statements, and **template bodies**
(the shape slick uses to assemble `SimpleFastPathResultConverter`). Rank 2
(`...$xss`) is still refused by name.

#### Two things fixed along the way

* **Empty `TypeTree`s inside expansions** (`crates/typer/src/expand.rs`).
  `q"val ff = $f"` makes nsc's quasiquote produce a `TypeTree()` that writes no
  type (the first two lines of `mapToImpl` are exactly this). Only in the type
  position of a `ValDef` is it lowered to `TreeKind::Empty` and the type left to
  inference. **Only there**, because our AST has no tree meaning "infer this"
  anywhere else, and we refuse as before in those places.
* **`_root_` did not resolve in term position** (`crates/typer/src/check.rs`).
  It was only handled in import paths, so
  `_root_.scala.collection.immutable.List(…)` — the shape macros write so as not
  to get caught in the caller's scope, and which slick's `mapToImpl` writes 11
  times — gave `not found: value _root_`. It now resolves to the root package.

#### Fixtures and tests

* `tests/fixtures/sv_impl.scala` + `tests/fixtures/sv_use.scala` — a macro
  implementation taking a refined `Context`, field enumeration via
  `decls.collect`, and mixed `..$` used in **three places** (argument clause,
  block, template body); it compiles in two stages and prints four lines. The
  same two files compiled in two stages by real scalac 2.13.16 and run produce
  **the same four lines** (`tests/fixtures/expected/sv_use.txt`). The
  template-body case puts **a string printing the tree it assembled** into the
  expansion, so if a splice lands in a different position the line changes (even
  though it still compiles and runs). The types being enumerated are **library**
  ones (`Deadline` / `BigDecimal`) because of remaining item 1 below, and
  `BigDecimal` has zero case accessors, i.e. it is the empty end of the
  concatenation.
* `tests/fixtures/sv_gaps_bad.scala` — the three shapes we refuse. Two of them
  (a rank-2 hole, a refinement that is not a `Context`) are **refused by real
  scalac too**, so they pin agreement; the third (a `case` class whose parents
  are `..$`) is a confession of something we have not implemented.

The tests are three appended to the end of `crates/cli/tests/engine.rs`
(`sv_refined_context_and_mixed_splices_run` /
`sv_refined_context_and_mixed_splices_match_real_scalac` /
`sv_refused_forms_are_named`).

#### Remaining

1. **scala-rs's own `ScalaSignature` does not record case accessors.** Macros
   read a `WeakTypeTag`'s members through the **runtime mirror**, so a case class
   compiled by scala-rs appears to have **empty** `decls` (applying `mapTo[R]` to
   an `R` built by scala-rs silently produces a zero-field expansion). This is
   why the fixture enumerates library types.
2. **A type pattern against an abstract type member becomes
   `instanceof java/lang/Object`.** The `TermSymbol` of `case s: TermSymbol` is
   an abstract type member of the universe, and `erase_ty` lowers it to `Object`
   (whereas type parameters are lowered to their upper bound). The test passes
   straight through, so expanding `mapToImpl` on a type whose `decls` contain
   non-`TermSymbol`s (e.g. `scala.io.Codec`) gives an
   `IncompatibleClassChangeError` at runtime. Fixing it requires emitting the
   type pattern's `instanceof` at the upper bound's erasure, which lands in
   codegen.
3. **A macro def read back from a scala-rs classfile is no longer a macro def.**
   `macro_impl` is not carried in the pickle, so calling `mapTo` in another run
   compiles into an ordinary method call and gives a `NoSuchMethodError` at
   runtime (with no diagnostic). Going through a real-scalac classfile is fine.
4. `_root_.scala.List` / `_root_.scala.Vector` give
   `no matching overload for <overload List$ | List$>`. The scope of the package
   `scala` contains two copies of the same companion, and lexical `scala.List`
   avoids them by a different path. Other names under `_root_`
   (`_root_.scala.Predef` / `_root_.scala.Some` / `_root_.java.lang.*` /
   `_root_.scala.collection.immutable.List`) work.
5. `mapToImpl` in `ShapedValue.scala` now **compiles**, but its **expansion**
   (the call site of `mapTo`) needs items 1–3 above plus the anonymous class in
   the expansion result (expand.rs cannot assemble a `ClassDef`). None of that is
   needed to compile slick itself.
