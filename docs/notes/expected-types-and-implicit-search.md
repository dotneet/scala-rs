# Expected types and implicit search

Development notes for the slices that worked on how an expected type flows into
arguments, how implicit parameter clauses get filled (and what happens when they
do not), and why implicit search sometimes failed to see a symbol at all. The
recurring theme is that a diagnostic saying "could not find implicit value" or
"value X is not a member of (…)Y" almost never points at the implicit machinery
itself; the root is usually one layer earlier — in how type arguments were
solved, how a qualifier was typed, or how a symbol entered the symbol table.

---

### Expected types are argument prototypes; a resolved overload's later clauses use the receiver's type arguments (`agent/cats3`)

Five cats-related errors that three earlier slices had left behind turned out to
have two unrelated roots, neither of them in `>>` or in the `Async`/`Deferred`
cascade they were blamed on: the expected type was never pushed into a by-name
parameter, and picking one alternative out of an overload set threw away the
receiver's type arguments.

Three slices (`agent/tail4` / `agent/cats2` / `agent/proj` / `agent/tail6`) had
been hunting for the root of five cats-related errors and left them behind:
three `no matching overload for (=> F[B]) (FlatMap[F])F[B]`
(`slick/basic/BasicBackend.scala`) and two
`could not find implicit value of type GenTemporal[F, _]`
(`slick/basic/ConcurrencyControl.scala`). This slice handled them. There were
**two separate roots**, and neither had anything to do with `>>` itself or with
the `Async` / `Deferred` cascade. A third issue was fixed along the way (an
implicit conversion whose own implicit clause was never completed).
`tests/slick_measure.sh` goes **`errors=99 → 92`, `files_with_errors=39 → 38`**
(zero new errors; what disappeared is the five above plus `Sync[F]` in
`slick/cats/Database.scala` and `FlatMap[F]` at `BasicBackend.scala:151`).
codegen (`crates/backend/`) was untouched, so `tests/slick_subset.sh` was
skipped.

#### 1. By-name formal parameters never became prototypes

nsc's `Infer.protoTypeArgs` solves the callee's type parameters **from the
expected type** before typing a single argument, and then **substitutes them
into the formal parameters**. `Checker::proto_arg_type` only did this when "the
formal parameter is a **bare** type parameter itself". cats'

```scala
def >>[B](fb: => F[B])(implicit F: FlatMap[F]): F[B]
```

has `=> F[B]` as its formal parameter, so it did not qualify, and the argument
was typed **with no expected type**.

```scala
a >> commitResult.fold(asyncF.raiseError, _ => asyncF.unit)
```

With no expected type, the `C` of `fold[C](fa: A => C, fb: B => C): C` becomes
`lub(F[A], F[Unit])` — `AnyRef` — which cannot possibly match `F[B]`, hence
`no matching overload for (=> F[B])(FlatMap[F])F[B] with arguments (AnyRef)`.
Solving `B = Unit` from the expected type `F[Unit]` and passing `=> F[Unit]`
pins `C` to `F[Unit]`, and the eta expansion of `asyncF.raiseError` is settled
with `A = Unit`.

If even one of the callee's type parameters survives the substitution, no
prototype is emitted (a leftover variable can only constrain the argument by its
bound, and that is the job of the later `open_to_bounds`). By-name is
**stripped** before passing it down: the expected type of an argument expression
is the value type, and re-wrapping into `Function0` is `adapt`'s job (passing it
still wrapped made `is_sub_type(F[Unit], => F[Unit])` false, so the caller's
"a prototype is a hint, not a constraint" retry threw it away).

The same path also removed one of the three errors (`BasicBackend.scala:432`)
**as a cascade**. `agent/tail4`'s reading — "this looks like a cascade from the
other six" — had the direction backwards: one of the three `>>` errors was the
cascade of the other two.

#### 2. The moment an overload was narrowed to one alternative, the receiver's type arguments were dropped

After picking one alternative out of an overload set, `type_apply_in` did:

```rust
if matches!(&fun.ty, Type::Overload(_)) {
    fun.ty = self.st.get(sym).ty.clone();   // ← the declaration itself
}
```

`fill_defaults_and_implicits` **re-reads the later (implicit) clauses from this
`fun.ty`**, so the implicit parameter's type reaches search still phrased in the
type parameters of the class that declared it. cats-effect's

```scala
final class GenTemporalOps_[F[_], A](val wrapped: F[A]) extends AnyVal {
  def timeoutTo(d: Duration,       fallback: F[A])(implicit F: GenTemporal[F, _]): F[A]
  def timeoutTo(d: FiniteDuration, fallback: F[A])(implicit F: GenTemporal[F, _]): F[A]
}
```

is **overloaded** on `Duration` / `FiniteDuration`, so the implicit clause of
`wait.timeoutTo(timeout, …)` arrives at search as a `GenTemporal[F, _]` whose
`F` is `GenTemporalOps_`'s own `F`, which can never match the `Async[F]` in
scope (the caller's `F`). Non-overloaded members carry the as-seen-from type
that `type_select` installed, so **only overloaded members** fell into this hole.

The fix reads the chosen alternative's type out of `overload_member_types` (the
type of each candidate as seen from the receiver) recorded by selection, and
puts that into `fun.ty`.

**`agent/tail6`'s diagnosis was wrong.** `E` is not being collapsed into
`Type::Wildcard`; the `_` in `GenTemporal[F, _]` is an existential **written
literally in cats-effect's source** (as `javap -s`'s `GenTemporal<F, ?>` shows,
`timeoutTo` has no type parameters at all). What was collapsing was `F`, not
`E`, and it had nothing to do with `cats.effect.syntax`'s implicit conversion or
with typing `Select`. `implicitly[GenTemporal[F, Throwable]]` worked while
`timeoutTo` did not, simply because the former is not overloaded.

#### 3. An implicit conversion's own implicit clause never let the candidate's parents be read

When search comes up empty, `fill_implicit_params_in` calls
`warm_implicit_candidates` and retries (`agent/tail6`). `fill_conv_implicits`,
which fills **an implicit conversion's** implicit clause, had no such retry.
Filling the `FlatMap[F]` of cats'

```scala
implicit def toFlatMapOps[F[_], A](fa: F[A])(implicit F: FlatMap[F]): FlatMap.Ops[F, A]
```

from `implicit val asyncF: Async[F]` (an **abstract** member of the trait)
requires reading `Async`'s parents, and search runs under an immutable borrow so
it cannot read them itself. It worked when another line in the same file
happened to have warmed `Async` up, and failed in isolation — the same shape
`agent/tail6` fixed, still present on the conversion side
(`connectionArbiter.allocateOrdinal.flatMap { … }` at
`slick/basic/BasicBackend.scala:151`). This is also why `implicit def` worked
where `implicit val` did not.

#### Fixtures and tests

* `tests/fixtures/c3_infer.scala` (+ `expected/`) — the two issues above placed
  side by side without using cats. It runs in both modes under `-Xverify:all`
  and matches the stdout of real scalac 2.13.16. **On main before the fix it
  fails with four errors.**
* `tests/fixtures/c3_infer_bad.scala` — that a prototype only guides inference
  and is not a licence to accept a value inferred earlier without an expected
  type (`type mismatch`), and that a witness for a different type constructor is
  still not found (`could not find implicit value of type TC[Box, _]`). A
  separate test pins that real scalac emits the same two errors on the same two
  lines.
* `cats_flat_map_then_and_timeout_to_compile` and
  `cats_syntax_conversion_completes_its_own_witness` (plus their scalac-side
  counterparts), which run only when cats is in the Coursier cache. The
  reproduction condition for the latter is that it be a **single compilation
  unit**: adding one line that touches `Async` makes it pass even before the
  fix.

The tests are nine of them in the new file `crates/cli/tests/cats3.rs`. What was
run, in `--release`: `cats3` / `cats2` / `catsyntax` / `catsimpl` / `tail6` /
`overloadshadow` / `ambigmap` / `setapply` / `uniteq` / `integral` /
`ordsummon` / `mutcoll` / `ovl2` / `ovl3` / `hkinfer` / `conform` / `e2e`
(all green, including the 460 e2e tests).

#### Remaining

* `BasicBackend.scala` went from 5 errors to 1. What remains is
  `type ExitCase is not a member of Resource$` (`Resource.ExitCase` is nested
  via cats-effect's package object — the same hole as `import` remaining item
  (a)).
* `ConcurrencyControl.scala` went from 3 to 1; what remains is
  `could not find implicit value of type Make[F]` (`Ref.of[F, State[F]](…)`).

---

### Implicit parameter clauses left unapplied in an expression's type, four roots (`agent/implclause`)

The symptom is an expression whose type prints as `(args)Result` — for example
`value isEmpty is not a member of (<:<[TermSymbol, (K, V)])Map[K, V]`. It was
tracked down from minimal reproductions. **Four independent roots hid behind the
same symptom**, three of them not in the "machinery that fills implicit clauses"
but **upstream of it** (how type arguments are solved, how qualifiers are typed,
how candidate eligibility is decided). slick goes
`errors=44 files_with_errors=26` →
`errors=40 files_with_errors=24` (zero new errors). Tests are in
`crates/cli/tests/implclause.rs`; fixtures are `tests/fixtures/implclause.scala`
(all cases in one file) and `tests/fixtures/implclause_bad.scala`.

**1. Align a function parameter's result to the parameter's class before solving type arguments.**

```scala
def h(v: Vector[(String, Map[Long, Int])]) = v.iterator.flatMap(_._2).toMap
```

`unify_one` **zips type arguments positionally without looking at the class
symbol**. Matching the lambda body's `Map[Long, Int]` against the
`IterableOnce[B]` of `flatMap[B](f: A => IterableOnce[B])` zips `[B]` with
`[Long, Int]`, giving `B = Long`, so `flatMap` returned `Iterator[Long]`. The
following `toMap[K, V](implicit ev: A <:< (K, V))` then looks for a witness for
`TermSymbol <:< (K, V)`, does not find it, and the method type survives as the
expression's type.

`unify_tparam_all` already aligned the **whole argument** to the parameter's
class via `align_to_param_class`, but did nothing when the parameter was a
function type. `align_arg_to_param` was added; it also aligns the **result** of
a function parameter. Only the result, because parameters are contravariant
(re-reading the argument side as a base class would throw away what the literal
actually wrote).

**2. A selection's qualifier fills its implicit clause even inside call arguments.**

```scala
def f(q: Qy[Int]) = SV(q.pack.to[Seq], "x")   // pack[R](implicit s: Sh[E, R]): Qy[R]
```

`adapt_implicit_apply` has an escape hatch (`typing_call_args`): "while
arguments are being typed before the overload is settled, do not touch the
clause". That is about the **argument trees themselves**, yet it was also
applying to the **qualifiers** inside them. `pack` has to be a value before `to`
can be selected on it, and nsc likewise types qualifiers in EXPRmode and adapts
them.

Dropping the flag for the **entire** time `type_select` types the qualifier went
too far, though. The same flag also decides how tag requests inside the
qualifier are answered, and dropping it wholesale made `weakTypeOf[ExBox[E]]` in
`tests/fixtures/ex_impl.scala` pick up `E`'s tag, printing `ExBox[ExRow]` as
`ExRow` (`--test engine` failed there). So the qualifier is typed **normally**,
and only when an implicit clause survived (`implicit_only_result`) is
`adapt_implicit_apply` applied a second time with the flag dropped. Along with
that, a clause that still could not be filled is handed to
`reject_unapplied_implicit_clause` right here — `adapt`'s backstop does not look
at qualifiers (there is no expected type). The message becomes
`could not find implicit value of type Sh[Int, R]` instead of
`value to is not a member of (Sh[Int, R])Qy[R]`.

**3. A class that inherits `A => B` is also material for solving type arguments.**

```scala
abstract class Conv[-A, +B] extends (A => B)
def flatten[R2](implicit ev: R <:< Act[R2]) = flatMap(ev)   // flatMap[R2](f: R => Act[R2])
```

`function_view` (the entry point that re-reads an argument as "the function type
it inherits") only looked at parents recorded as the **class** `Function1`. A
parent written `extends (A => B)` is recorded as `Type::Function` (and `<:<`
enters in that shape too). Conformance itself worked — you can write
`val g: R => Act[R2] = ev` — but solving the callee's `R2` from the argument
came up empty, producing `no matching overload`. `Type::Function` parents are
now returned as the view directly.

**4. A derivation rule with `ClassTag` in its implicit clause is not an "unusable candidate".**

```scala
implicit def forColl[C[X] <: Iterable[X]](implicit cbf: Factory[Any, C[Any]],
                                          tag: ClassTag[C[Any]]): Coll[C]
implicitly[Coll[Seq]]   // ← was not found
```

`implicit_fit_at` decides whether a derivation rule is usable by asking whether
its own implicit arguments are **found** by `search_implicit_at`. As in nsc,
`ClassTag` / `TypeTag` are **manufactured, not searched for**, so they always
failed this check. Writing `implicitly[ClassTag[Seq[Any]]]` directly worked
while putting it inside a rule did not. The fallback that `fill_implicit_params`
already had was given to the eligibility check too (`built_not_found`), and the
same fallback was added to the recursion in `implicit_tree`, which builds the
tree. The view-style fallbacks (`identity_view` / `conversion_view` and
friends) were **not** included — those run searches of their own, which would
make every function-typed parameter count as "fillable". This is slick's
`Query.to[Seq]` (`TypedCollectionTypeConstructor[Seq]`).

**Claims from the brief that measurement refuted:**

* The implicit clause of `xs.flatten` (raised by `agent/probe12`) is **already
  fixed on main** (merge `cbf207b`). `List(Some(1), None).flatten.sum` compiles
  on current main.
* `implicitly[String => String]` being crushed by
  `reject_unapplied_implicit_clause` (raised by `agent/dbio`) **does not
  reproduce** either. It works now.
* The story about `Predef.$conforms` is correct as it stands, and the real jar's
  `javap scala.Predef$` does show
  `public <A> scala.Function1<A, A> $conforms()` (exactly as described in
  `crates/typer/src/prelude_conform.rs`). Since the implementation returns
  `<:<.refl`, a search for `A <:< B` landing on `<:<.refl` is the same behaviour
  as nsc; it was never a scope-construction ordering problem.

**Remaining** (minimal reproduction in hand, not fixed):

* `Array[T]` is **not converted to `IterableOnce[T]` in argument position**.
  Both `Map() ++ arr` (slick `jdbc/JdbcTypesComponent.scala:526`) and
  `def f[B](x: IterableOnce[B]); f(arr)` give `no matching overload`. Writing
  `arr.toSeq` works. scala-rs supports `Array` through member supply from
  `ArrayOps`, and the root is that there is **no general view** corresponding to
  `Predef.wrapRefArray` / `genericWrapArray`. Filling it needs the codegen side
  to insert a wrap, which is a different slice from this one (type checking
  only).

#### Tests and measurements

`cargo test --workspace --release` is green across all 118 binaries
(`implclause` adds one, 117 → 118). The seam list (`overloadshadow` /
`ambigmap` / `setapply` / `uniteq` / `integral` / `ordsummon` / `mutcoll` /
`conform` / `e2e`) plus `mismatch14` / `hkinfer` / `dbio` / `buildfrom` /
`buildfrom2` / `arrconv` / `seqfn` / `cats2` / `cats3` / `catsimpl` / `reject` /
`ovl3` / `ovl4` / `proj` / `asttype` / `engine` were also run individually.
`crates/backend/` was untouched, so `tests/slick_subset.sh` was skipped. slick
under `tests/slick_measure.sh` went from `errors=44 files_with_errors=26` at the
start to `errors=40 files_with_errors=24` at the end; what disappeared is the
four errors at `compiler/CreateAggregates.scala:99,100` /
`dbio/DBIOAction.scala:52` / `lifted/Query.scala:191`, and **nothing was added**.

---

### 13 "not found / not visible" errors, seven roots (`agent/implfind`)

The four remaining "implicit not found" errors and two "cannot access member"
errors in slick, plus seven one-off errors of the same family, were reduced to
minimal reproductions; there were seven roots. **Not one diagnostic's wording
matched its root** — the first, for instance, was a subtyping bug, not an
implicit search bug. Every case was checked to be accepted by real scalac 2.13.16
(`/tmp/scala-2.13.16/bin/scalac`) before being fixed. Tests are in
`crates/cli/tests/implfind.rs`; fixtures are `tests/fixtures/implfind.scala`
(all cases in one file) and `tests/fixtures/implfind_bad.scala` (the flip side
of the relaxed access rules).

slick: `errors=44 files_with_errors=26` → **`errors=31 files_with_errors=22`**
(13 fewer, zero new).

**1. An applied abstract type member did not conform to its own upper bound.**
The root of the three "implicit not found" errors (`TypedType[Boolean]`,
`JdbcType[U]`, `JdbcType[U] with BaseTypedType[U]`) was not implicit search but
**subtyping**.

```scala
trait TT[T]
trait C { type CT[T] <: TT[T] }
def d[U](implicit ev: C#CT[U]): TT[U] = ev   // this was a type mismatch
```

`is_sub_type`'s "`Applied` vs anything else" rule compared the abstract type
member's upper bound (`bound_hi`) against the other side **still phrased in the
member's own parameters**. The upper bound of `CT[U]` is `TT[U]`, not `TT[T]`.
Because it was not substituted with the applied arguments, `CT[U] <: TT[U]` was
always false, which put the evidence introduced by a context bound in the state
of **not satisfying its own bound**. `crates/typer/src/symbol.rs`.

**2. A context bound's evidence type was not expanded through the self type.**

```scala
trait JComp extends Comp { self: JProf =>
  def base[U : BCT](u: U) = implicitly[BCT[U]]   // evidence is the only candidate, yet it mismatches
}
trait JProf extends Prof with JComp { type BCT[T] = JT[T] with BB[T] }
```

`[U : BCT]` writes its bound as a **bare name**, so it never goes through
`tree_to_type`'s "apply arguments to a type constructor" path (which calls
`expand_type_members` last). The body's `implicitly[BCT[U]]` becomes
`JT[U] with BB[U]` through the self type, while the evidence alone stayed as the
abstract `BCT[U]`, so the only candidate did not match the requirement.
`Checker::expand_bound_evidence` (both `class_bound_evidence` and the `def`
side).

**3. `protected` members of a companion `object`.**
nsc's `Contexts.isAccessible` first checks `accessWithin(ab) || accessWithinLinked(ab)`
(`ab = sym.owner`). **Inside the owner, or inside its companion**, no subclass
rule is needed even for `protected`. scala-rs only consulted
`protected_subclass_ok`, so slick's

```scala
trait ResultConverterCompiler[R, W, U] { … ResultConverterCompiler.logger … }
object ResultConverterCompiler { protected lazy val logger = … }
```

produced `value logger cannot be accessed`.

**4. Nested `private[pkg] object` / `class`.**
`namer_enter_tmpl` **did not record** `private_within` for `ClassDef` /
`ModuleDef` (it did for `val` / `def` / `type`). Qualified private was treated
as plain private, so slick's `private[jdbc] object GetUpdateValue` (inside
`object GetResult`) was invisible to `SQLActionBuilder` in the same package.
The brief's reading — "touching a companion's private from outside" — was wrong;
this is a **dropped qualified private**, unrelated to companions.

**5. Self aliases on anonymous classes.** `parse_new` was discarding the `base`
of `new T { base => … }` (`self_name: None`, hardcoded). slick `TableQuery`'s
`not found: value base` is nothing but this.

```scala
val baseTable = cons(new BaseTag { base =>
  def taggedAs(path: Node) = cons(new RefTag(path) {
    def taggedAs(path: Node) = base.taggedAs(path)   // ← not found: value base
  })
})
```

**6. In the function position of a constructor pattern, a non-stable `def` is not a candidate.**
nsc's `Context.lookupSymbol` excludes `sym.isMethod && !sym.isStable` when
`typingConstructorPattern`. slick's `Node` is written as

```scala
final def :@ (newType: Type): Self = …          // a method on Node
import slick.ast.TypeUtil.*                     // object TypeUtil { object :@ { def unapply … } }
val from2 :@ CollectionType(_, el) = from.infer(scope, typeChildren): @unchecked
```

so the **inherited method `:@` shadowed the imported extractor `object :@`**,
giving `not found: extractor :@`. It did not happen inside `case` and only
appeared in `val` pattern definitions, because
`case (LiteralNode(lv) :@ (lt: TypedType[?]), …)` is written in a class that
does not inherit from `Node`. `SymbolTable::lookup_extractor` and
`Checker::ctor_pattern_fun`. This also removed the two `<notype>` cascades in
`Node.scala`.

**7. A Java `Object` return type is `AnyRef`, not `Any`.**
nsc's `objToAny` is only called in the ClassfileParser's parameter loop. Return
types stay `AnyRef`, so `eq` / `ne` / `synchronized` are available. scala-rs
turned `java/lang/Object` into `Type::Any` uniformly, so
`if(cv.unwrapped eq null)` against typesafe-config's `ConfigValue.unwrapped():
Object` (slick `GlobalConfig`) gave `value eq is not a member of Any`.

A version faithful to nsc — `Any` for parameters only, `AnyRef` everywhere else
(returns, fields, type arguments) — was also tried, but it rewrote **type
arguments** such as `Hashtable<Object, Object>` too and **introduced one new
error** in slick's `HeapBackend`, where `IndexedSeq[Any] <: Int => Any` fails
(net zero). Since that widening gained nothing on slick, it was kept to the
**top level of return types only** (`java_result_obj`). What remains is the
`Object`-in-type-arguments side.

**8. (Side effect) `scala.collection.Map` had no members.**
The "link" traits built by `prelude_hier` carry no members, and `get` /
`contains` / `getOrElse` / `apply` existed only on the `immutable.Map` /
`mutable.Map` side. In 2.13 all of these are declared on
`scala.collection.MapOps`, so the abstract side is where they belong. slick's
`ExpandTables` emitted three errors against an argument received as
`collection.Map`:

```
value contains is not a member of Map[TableIdentitySymbol, (TermSymbol, Node)]
no matching overload for ((K, V)*)Map[K, V] with arguments (TableIdentitySymbol)
value replace is not a member of B
```

All one root (`expansions(tsym)` picked up the **companion's `Map.apply`**,
making `exp: B`, so `exp.replace` looked for a member of `B`).
`crates/typer/src/prelude_implfind.rs`.

**9. (Side effect) A pickle-derived nested class resolved to the companion in type position.**
Reading `object Ref { trait Make[F[_]] }` from a classfile splits it in two: the
pickle puts `Make`'s **module accessor** on `Ref$`, while the trait, whose name
`Ref$Make` alone does not say which `Ref` it is nested in, is placed by
`find_or_stub_java_class` under the **trait `Ref`**. `lookup_qualified_type`
stopped at the first owner that matched, so `Ref.Make[F]` resolved to the object
and gave `Make does not take type parameters`. It now **prefers a class over an
object** across owners (`fs2.Stream.ToPull[F, O]` is the same case).

**Remaining** (with minimal reproductions):

* `no implicit: could not find implicit value of type Make[F]`
  (slick `basic/ConcurrencyControl.scala:202`, `Ref.of[F, State[F]](…)`).
  Root 9 made `Ref.Make[F]` work as a type, and **finding an
  `implicit mk: Ref.Make[F]` in scope via `implicitly`** now works too. Two
  things remain. (a) The **insertion of the implicit argument** for
  `Ref.of[F, Int](0)` does not happen (writing `Ref.of[F, Int](0)(mk)`
  explicitly works). (b) The

  ```scala
  implicit def concurrentInstance[F[_]](implicit F: GenConcurrent[F, ?]): Make[F]
  ```

  that `Ref.Make`'s companion inherits is not usable from the implicit scope
  (matching `Concurrent[F] = GenConcurrent[F, Throwable]` against the
  existential `GenConcurrent[F, ?]` is required). Minimal reproduction:

  ```scala
  import cats.effect.kernel.{Concurrent, Ref}
  def d[F[_]](implicit mk: Ref.Make[F]): F[Ref[F, Int]] = Ref.of[F, Int](0)   // (a)
  def k[F[_]](implicit F: Concurrent[F]): Ref.Make[F] = implicitly[Ref.Make[F]] // (b)
  ```

* `type ExitCase is not a member of Resource$` (slick
  `basic/BasicBackend.scala:421`). **Does not reproduce in isolation.**

  ```scala
  import cats.effect.{Async, Ref, Resource}
  import cats.effect.kernel.Outcome
  import cats.syntax.all.*
  import cats.effect.syntax.all.*
  object C { def ec(e: Resource.ExitCase): String = e.toString }   // this compiles
  ```

  Even lining up the same imports as BasicBackend.scala compiles, so it only
  breaks when the whole of slick goes through in one run (the ordering of member
  completion on `Resource$` is suspected but unverified). The brief's hypothesis
  — "a nested class reached through a package object's `val`" — does **not hold,
  at least in isolation** (`cats.effect.Resource` is exactly that shape and
  compiles on its own).

**Where the brief was wrong:**

* "The two access errors are companion-object private/protected members touched
  from outside the companion class" — the `GetResult.GetUpdateValue` one has
  nothing to do with companions; scala-rs simply was not recording the
  **qualified private** `private[jdbc]`. The prefix computation is correct, and
  touching it from `SQLActionBuilder` is a legitimate shape.
* "`TypedType[Boolean]`'s candidate is an implicit in slick's `TypedType`
  companion or in the profile cake" — the candidate is
  `booleanColumnType: BaseColumnType[Boolean]` via `api`, and it **was** in
  scope. What failed was deciding
  `BaseColumnType[Boolean] <: TypedType[Boolean]` (root 1).
* "`value eq is not a member of <notype>` is a sign the type was never computed"
  — correct, but the upstream was neither `eq` nor `Any`; it was the extractor
  resolution of `:@` two lines above on the same line (root 6).

---

### The three cats-effect errors — what "does not reproduce in isolation" really was (`agent/final2`)

The three remaining cats-effect errors in slick (`Resource.ExitCase`,
`Ref.Make[F]`, `cats.effect.IO(fa)`) were fixed. slick goes
`errors=17 files_with_errors=13` → **`errors=13 files_with_errors=10`**
(`tests/slick_measure.sh`; files that lost errors: `basic/BasicBackend.scala`,
`basic/ConcurrencyControl.scala`, `dbio/DBIOAction.scala`; along the way the one
`Column$` error in `JdbcModelBuilder.scala` disappeared too). Fixtures are
`tests/fixtures/f2_cats.scala` (the accepting side, all cases in one file) and
`tests/fixtures/f2_cats_bad.scala`; the tests are in
`crates/cli/tests/final2.rs`. On main before the fix (`d7e7767`) this one file
produces five errors.

Two of the three had been reported by three slices as "only breaks when the
whole of slick is compiled". **The roots all have the same shape**: a symbol
enters the symbol table through **another path** before the program writes its
name, and the answer given by whoever got there first is the one that survives.
So the first thing done was to identify that "earlier path" and **fold it into a
single file** (see "How to reproduce", below).

#### 1. `Ref.of`'s `implicit mk: Ref.Make[F]` was not found

`ConcurrencyControl.scala:202`. **This one does reproduce in isolation** (the
previous slice's reading — "(a) implicit-argument insertion" and "(b) the
implicit scope of the existential `GenConcurrent[F, ?]`" — was **wrong on both
counts**).

```scala
def create[F[_]](n: Long)(implicit F: Async[F]): F[Ref[F, Long]] = Ref.of[F, Long](n)
```

The only candidates for `Make[F]` are
`Ref.MakeInstances#concurrentInstance` / `MakeLowPriorityInstances#syncInstance`,
inherited by `Ref.Make`'s companion. Under `SCALA_RS_IMPL_DEBUG` (a trace added
temporarily for the investigation) the candidate set was empty. The cause is in
`Check::load_companion_module`, which was entering
`cats/effect/kernel/Ref$Make$` into the **package** `cats.effect.kernel` under
the name `Make`. `SymbolTable::companion_module` looks for a module of the same
name "among the members of the class's own owner", so it went looking at `Make`'s
owner `Ref` and found nothing. Writing `Ref.Make` **in the source** creates the
companion by another path and makes it work — hence the apparent order
dependence. The fix is a one-liner restoring the intended owner:

```rust
// load_companion_module: the companion of a nested class belongs to whatever
// encloses that class, not to the package.
let owner = {
    let o = self.st.get(class_id).owner;
    if !o.is_none() && self.st.get(o).is_class_like() { o }
    else { crate::classpath::ensure_package(&mut self.st, pkg) }
};
```

#### 2. `type ExitCase is not a member of Resource$`

`BasicBackend.scala:421`. **Here is the reproduction**: the same file has to
mention `fs2.Stream` by name.

```scala
def stream(s: fs2.Stream[cats.effect.IO, Int]): Int = 0
def succeeded(e: Resource.ExitCase): Boolean = e == Resource.ExitCase.Succeeded
```

Reading `fs2/Stream.class` touches `cats/effect/kernel/Resource$ExitCase` in its
member descriptors. A nested classfile `Outer$Inner` says nothing about whether
`class Outer` or `object Outer` declared it, so `classpath::java_class_owner`
**always answers with the class**. As a result `ExitCase` enters as a member of
the **trait `Resource`**, and the source's `Resource.ExitCase` (the path through
the `Resource` **object**) looks in `Resource$` and finds nothing. Compiling
`BasicBackend.scala` alone gives the reverse order (`Resource$` is read first),
which is the only reason it worked.

The fix adds `enter_in_companion_scope` to `classpath::install_java_class_in`:
"if the owner being asked about is the companion module class of the owner we
currently hold, enter the same symbol into that scope as well". No symbols are
added and no owners are rewritten; both spellings simply reach **one and the
same class**.

Note that `complete_type_member` in `pickle_supply.rs` **memoizes** this `None`
in `tried_types`, so once it fails it fails forever after. A
`SCALA_RS_PICKLE_DEBUG=1` trace was added at that entry point
(`… : no pickle read -- the class has not been adopted yet`). Order-dependent
"type X is not a member of Y$" starts here.

**Correcting the previous slice's reading**: "a nested class through a package
object's val" does not hold, as `agent/implfind` pointed out. But it is not
"duplicate supply" either — it is that **the owner of a nested class cannot be
decided from the classfile name** (class or companion?).

#### 3. `cats.effect.IO(fa)` gives `no matching overload`

`DBIOAction.scala:237`. This too reproduces in isolation once `fs2.Stream` is
written in the same file. `IO.apply(thunk: => A): IO[A]` takes a **by-name
argument**, which cannot be written in a classfile's generic signature
(`(Lscala/Function0<TA;>;)…`). The classfile reader's copy comes out as
`apply(Function0[A]): IO[A]`, which nothing of type `Future[R]` matches. Worse,
completion from the pickle only runs "when `lookup_member` found **nothing at
all**", so as long as that wrong copy exists it can never be repaired. scalac
also emits each companion method as a **static forwarder on the class side**, so
the same erased `apply` lands on `cats/effect/IO` too (and that is the one that
was being chosen here).

`Check::retry_module_apply_from_pickle` was added. It runs **only immediately
before emitting a `no matching overload`**, completes `apply` from the pickle on
the receiver's companion module class, and re-types the tree. If nothing new
enters it returns `false`, so there is no recursion. It does **not** adopt the
companion eagerly: adopting `IO$` triggers completion of ~200 members and takes
minutes on a six-line source (as documented in `supply_implicit_members`'s doc
comment).

#### How to reproduce (for the next person who hits this shape)

* **When you suspect order dependence, look at where the symbol was created.**
  Adding one line — `std::backtrace::Backtrace::force_capture()` — to
  `find_or_stub_java_class` and running slick once end to end names the culprit
  that created `Resource$ExitCase` on the first try: `fill_java_members` (i.e.
  when `fs2/Stream.class` was read). From there, writing "the one line that
  forces that classfile to be read" gives you an isolated reproduction.
* **Bisecting the file set was not necessary.** Rather than whittling down 184
  files, looking directly at "who created that symbol first" is faster (one full
  run ≈ 90 seconds; a bisection is at least eight runs).
* **Within one file, signature resolution happens before bodies are typed.** So
  warm-up of the form "touch another member first" never lands before
  `Resource.ExitCase`, whether in the same file or in two files. The only thing
  that works is forcing a classfile to be read during `parents_pass` (i.e.
  writing the name in type position).

#### Remaining (not fixed here)

* When a method with an implicit parameter clause is referenced **explicitly**
  with an expected type given, the implicit argument is not inserted. It does
  not show up in slick, but it is the same area.

  ```scala
  def a3[F[_]](implicit F: Async[F]): Ref.Make[F] = Ref.Make.concurrentInstance[F]
  // type mismatch; found: (GenConcurrent[F, _])Make[F]  required: Make[F]
  ```

  Both `implicitly[Ref.Make[F]]` and `Ref.of[F, Long](n)` work, so this is not
  implicit search itself but the "apply the clause to an explicit reference"
  side.
* `cats.effect.IO` sometimes **resolves to the class symbol in term position**
  (when `IO$` is not yet in the symbol table). The fix in 3 works even from that
  state, but it ought to resolve to the module; fixing that would remove the
  choice of a static forwarder in the first place.
* nsc rejects `IO(1, 2)` with `too many arguments`, while we auto-tuple it into
  `IO[(Int, Int)]` (a difference inherited from main).
  → All three of the above were fixed in the next section (`agent/arraygen`).

---

### An existential's bound is data the search needs (`agent/slickshape`)

Two roots behind gitbucket's slick DSL, `errors=1736 → 1588` on
`tests/gitbucket_measure.sh` (−42 and −106, additive). Full write-up in
`docs/gitbucket.md`, roots 20 and 21; the parts worth having here:

**1. A wildcard in the wanted type is not always "a position the search is not
asking about".** `Unify::unify_at` answers `true` for `Type::Wildcard` on
either side without binding anything, which is right for `List[_]` and wrong
the moment the candidate has a type parameter standing opposite it and nothing
else to solve it from. slick's

```scala
def map[F, G, T](f: E => F)(implicit shape: Shape[_ <: FlatShapeLevel, F, T, G]): Query[G, T, C]
```

is answered by `repColumnShape[T : BaseTypedType, Level <: ShapeLevel]`, whose
`Level` can *only* come from that first position. A `BoundedWildcard` binds it
(to the bound); a bare `Wildcard` does not, and `implicit_solve` then drops the
candidate rather than guessing. `PickleSupply::conv_at` was flattening every
quantified variable of an `Existential` to `Type::Wildcard`, so a bound written
in a jar was lost while the same bound written in source (`subst_quantified`)
was kept. **When a diagnostic prints `_` where the source says `_ <: X`, the
type has already lost the only thing that could answer it.**

**2. `candidate_bounds_hold` is a subtype question, so it needs parents.** With
the bound restored, the next question is whether `_ <: FlatShapeLevel` is a
`ShapeLevel` — and `FlatShapeLevel` is a jar class that appears *only* inside
slick's own signature, so nothing had ever read its parents and the answer was
no. This is the `warm_implicit_candidates` shape from `agent/tail6` and
`agent/cats3` again, one step over: the class that needs warming is named by
the **wanted type**, not by any candidate. `warm_implicit_candidates` now takes
the types the search came up empty on, and `collect_type_parts` follows a
`BoundedWildcard`'s bounds (nsc's `companionImplicitMap` follows an abstract
type's `bounds.hi` for the same reason).

The tell for both is the same one this file keeps recording: adding a line that
merely *names* the type (`def warm(x: FlatShapeLevel): ShapeLevel = x`) made
the file compile. That is always a missing completion, never a scoping rule.

**3. `search_extension` compares conversions; nsc compares members.** Two
conversions that both offer `&&` on `Rep[Boolean]` — slick's one-argument one
and gitbucket's `implicit class RichColumn(c1: Rep[Boolean]) { def &&(c2: =>
Rep[Boolean], guard: => Boolean) }` — tied on every rule we have
(declared-vs-inherited, low priority, argument specificity), because they are
genuinely equal *as conversions*. nsc's `adaptToArguments` asks for a view
whose result has a member applicable to the arguments, and a two-argument `&&`
is not one for `a && b`. `Check::callee_arity` carries the enclosing `Apply`'s
argument count into the selection so `drop_inapplicable_conversions` can
narrow the tie; it only ever narrows, and a member whose shape cannot be read
stays a candidate.

---

### A candidate's own clause is where its leftover parameters come from, and a cake's type member has parameters too (`agent/slickimplicit`)

Two roots under gitbucket's remaining slick implicit clusters,
`errors=1399 → 1276` on `tests/gitbucket_measure.sh` (−19 and −104,
additive). Full write-up in `docs/gitbucket.md`, roots 26 and 27; the parts
worth having here:

**1. "The wanted type does not pin this parameter down" is not the same as
"nothing can".** `implicit_fit_open` exists precisely to settle a candidate's
type parameters from the candidate's *own* implicit clause, and it was gated
on the **call site** having left something undetermined. slick's

```scala
implicit def tuple2Shape[Level <: ShapeLevel, M1, M2, U1, U2, P1, P2](implicit
  u1: Shape[_ <: Level, M1, U1, P1], u2: Shape[_ <: Level, M2, U2, P2]
): Shape[Level, (M1, M2), (U1, U2), (P1, P2)]
```

answered against `Shape[_ <: FlatShapeLevel, T, U, _]` has `P1`/`P2` opposite
a bare `_` — nothing at the call site is undetermined, and nothing on the
wanted side can say what they are, but `u1` and `u2` can. Dropping the gate is
one line and worth 19 in gitbucket; every other guard the fallback has (open
after the clauses, no witness, bounds, conformance) is what keeps it honest,
and `neg` did not move.

The limit that remains is worth remembering: **`Unify` keys its unknowns by
symbol id**, so a rule that derives *itself* has the candidate's own `P1` and
the caller's open `P1` as the same symbol, and the occurs check rejects
`P1 := (P1, P2)`. nsc gives every application fresh type variables. Nested
tuple shapes are still not found for that reason.

**2. An abstract type member can take type parameters, and slick's cake is
written entirely of them.** `type BaseColumnType[T] <: ColumnType[T] with
BaseTypedType[T]` is pickled as a `PolyType` over the bounds;
`abstract_type_member` read the bounds and dropped the parameters, so every
use said "does not take type parameters" and the bound that survived mentioned
a free `T`. Two adjacent gates hid how much that cost:
`conv_ref` only offered a bare `Ref` to `self_type_member` when it had **no
arguments**, and `self_type_member` itself ran only for `scala.*` classes.
Together they made `BaseColumnType[Boolean]` — the declared type of all
twenty-four of slick's column types — an unmappable result type.

The tell was in the trace and not in the diagnostic: `has_pickle
BaseColumnType: NotFound("BaseColumnType")` for a name that is not a class and
never will be. **When a pickle's `Ref` has a bare name, it is a type member of
the cake being completed**, and that is true whether or not it carries
arguments.

What is still missing is the *prefix*: `RelationalTypesComponent
.BaseColumnType[T]` means the abstract declaration when read on its own and
`JdbcProfile`'s concrete alias when seen from a real profile, and this reader
has no as-seen-from for a type member through a path. So a value can be
*declared* at the member (its bound makes it a `TypedType[T]`) but nothing can
be shown to conform *to* it, which is one error left in gitbucket at
`MappedColumnType.base[java.util.Date, java.sql.Timestamp]`.

**Unrelated, found on the way and not fixed here:** selecting a member off
`Predef.implicitly[X]`, or assigning it to a field, emits bytecode with no
`checkcast`, and the JVM verifier rejects it (`implicitly[Box[String]].show`
is enough). Our own `def summon[T](implicit e: T): T` gets the cast; the
library's does not.
