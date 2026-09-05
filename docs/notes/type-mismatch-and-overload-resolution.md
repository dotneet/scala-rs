# Type mismatches and overload resolution

These notes cover the batches of `type mismatch` and `no matching overload` errors that were still left when compiling slick and cats-effect, and the prelude/library signature gaps that turned out to be behind most of them. The recurring theme is that a single missing or over-simplified signature in the prelude looks, from the outside, like a dozen unrelated inference bugs. Topics here include the collection hierarchy (`Seq` really is a `PartialFunction`), summoner methods whose result type is `F.type`, solving type variables through lambda results and inherited members, missing overloads and missing parent edges in the prelude, getting expected types down into function literals in argument position, type projections `A#B`, what a `package` clause opens, and where default arguments get typed.

### `Seq` is an `Int => A` (`agent/seqfn`)

The bug: passing a `List` where an `Int => A` was expected failed with `type mismatch`. The root cause was a missing edge in the prelude's class hierarchy -- `Map <: Function1` was already wired up, but the corresponding `Seq <: PartialFunction[Int, A]` edge, which exists in the real 2.13 library, had simply never been added.

```scala
val s = List(10, 20, 30)
println(List(0, 2).map(s))            // List(10, 30) -- pass List as an Int => A
val f: Int => Int = List(10, 20, 30)  // assignment works too
List(1, 2).isDefinedAt(5)             // false
```

This was reporting `type mismatch; found: List[Int]  required: (Int) => Int`.
The edge for passing a `Map` as a function (`crates/typer/src/prelude_mism4.rs`,
`Map[K, V] <: Function1[K, V]`) was already there; only the `Seq` side was missing.

In 2.13, `scala.collection.Seq[A]` inherits `PartialFunction[Int, A]` (and therefore
`Int => A`) in its own declaration (`javap scala.collection.Seq`):

```text
public interface scala.collection.Seq<A> extends scala.collection.Iterable<A>,
  scala.PartialFunction<java.lang.Object, A>, scala.collection.SeqOps<...>, scala.Equals
```

`Map` was given `Function1` as a direct parent (there was a known reason for this: putting
`PartialFunction` in between breaks type checking of `toMap`). That reason does not apply to
`Seq`, so this time the true hierarchy `Seq <: PartialFunction[Int, A] <: Function1[Int, A]`
was wired up in `crates/typer/src/prelude_seqfn.rs` (a new file). The edge is attached at
exactly one place, `scala/collection/Seq` -- the common ancestor, assembled by
`prelude_hier.rs`, of `List` / `Vector` / `ArraySeq` / `Range` / `LazyList` / `Queue` /
`mutable.Seq` (including `Buffer` / `ArrayBuffer` / `ListBuffer`) and the rest -- and it
propagates to every concrete collection below through `base_type_seq`'s transitive parent walk.

Because `PartialFunction` is now a parent, `Seq` inherits `lift` / `orElse` on top of
`isDefinedAt` / `applyOrElse` (`lift` and `orElse` were not in `add_partial_function` before,
so they were added in the same file). This means `Seq[A]` now has two `apply` members that can
only be told apart after instantiation -- `SeqOps.apply(Int): A` and
`PartialFunction[Int, A].apply(Int): A` -- but plain indexing such as `s(1)` / `s.apply(2)`
still resolves to `List`'s own concrete `apply` (via the existing machinery in
`overload_member_types`) and still emits
`invokeinterface scala/collection/SeqOps.apply`; only the sites where the value is passed as a
`Function1` emit `invokeinterface scala/Function1.apply`.

`Array` is not a `Seq` itself: it only reaches `Seq` through the **implicit conversion**
`Predef.wrapBooleanArray: Array[Boolean] => mutable.ArraySeq[Boolean]`
(`List(0, 2).filter(anArrayOfBoolean)` / `(2 to 30).filter(sieve)`). `wrapBooleanArray` was
missing from the prelude entirely, so it was added (in `prelude_seqfn.rs`), with its return
type matching the descriptor in the real jar (`scala/collection/mutable/ArraySeq$ofBoolean`).
A version returning the `mutable.ArraySeq` trait itself type-checks but fails to link at run
time with `NoSuchMethodError`; `mutable.ArraySeq` had been hand-built by `prelude_mutcoll.rs`
with only `AnyRef` as a parent, so the edge to `Seq`'s ancestor `mutable.IndexedSeq` is added
here as well. For the same reason as `wrapIntArray` it is not marked `IMPLICIT` (to avoid
competing with `xArrayOps`). But since `wrapXArray` is an implicit conversion and not
subtyping, dedicated hooks are needed in both `arg_score` (whether an overload candidate is
applicable) and `adapt` (building the actual call tree) -- this is the new file
`crates/typer/src/seqfn_view.rs`.

All of this is `library_abi`-only. In the private runtime (`--no-scala-library`,
`crates/backend/src/runtime.rs`), `scala/PartialFunction` is an abstract interface with only
`isDefinedAt` / `applyOrElse`, with no default implementations of `lift` / `orElse`, and the
private classfiles for `List` / `Vector` and friends do not implement `scala/PartialFunction`
or `scala/Function1`. To avoid emitting a broken link -- an `invokeinterface` to a target that
type-checks but has no implementation -- the non-jar mode continues to report `type mismatch` /
`value isDefinedAt is not a member of ...` exactly as before.

#### Verification

The fixture prefix is `sf`; the tests are in `crates/cli/tests/seqfn.rs`.

| fixture | contents | expected |
|---|---|---|
| `sf.scala` | passing `List` / `Vector` / `mutable.ArrayBuffer` as an `Int => A` in both assignment and argument position, covariance (`List[Dog] <: Int => Animal`), `isDefinedAt` / `lift` / `orElse`, `String` via `wrapString`, `Array[Boolean]` via `wrapBooleanArray` (both assignment and `filter`) (library mode, `java -Xverify:all`, expected output taken verbatim from real scalac 2.13.16's stdout) | `20` `List(10, 30)` `c` `7` `Rex` `true` `false` `Some(2)` `None` `1` `-1` `c` `true` `false` `List(0, 2)` `List(0, 1)` |

`sf.scala` is driven from `seqfn_fixture_dual_run`. The same file also holds minimal
acceptance tests (`a_list_is_usable_as_int_to_a` /
`partial_function_members_reach_list_without_upstaging_its_own_apply` /
`vector_indexed_seq_and_array_buffer_are_all_usable_as_functions` /
`a_string_is_usable_as_int_to_char_via_wrapped_string` /
`a_boolean_array_is_usable_as_int_to_boolean` /
`a_list_of_a_subtype_is_usable_as_int_to_the_supertype`).
Conversely, that the relaxed rule does not swallow diagnostics is pinned down by
`sf_bad.scala` (passing a `List[Int]` where a `String => Int` is required, and passing a
`List[Animal]` to an `Int => Dog` -- the direction where covariance does not help), via
`sf_bad_is_still_rejected`. Real scalac 2.13.16 rejects both as well.
That `--no-scala-library` still produces the old diagnostics is pinned down by
`without_the_library_the_old_diagnostics_still_fire`.

#### Remaining

- `Set[A] <: A => Boolean` also genuinely exists (`SetOps` inherits
  `Function1[A, Boolean]`), but this edge was not added -- the same judgment call that
  `prelude_mism4.rs` made for `Map`, to limit the fallout on overload resolution and implicit
  search.
- `Predef.wrapXArray` for element types other than `Boolean` was not added in this slice
  (`Int` has an existing `wrapIntArray`, but it only returns an `ArraySeq$ofInt` that is not
  connected to `Seq`; `Byte` / `Short` / `Char` / `Long` / `Float` / `Double` / `Unit` and
  reference types are missing entirely). Passing an `Array[Int]` as an `Int => Int` is
  accepted by real scalac but still reports `type mismatch` here.
- The conversion that lets an `Array` be passed as a function goes through dedicated hooks in
  `arg_score` / `adapt` (`seqfn_view.rs`); it is not a general "also try implicit views in
  argument position" mechanism. `arg_score` originally decided using `is_sub_type` alone, so
  the same class of hole -- implicit conversions in general not applying in argument position
  -- may well remain elsewhere.
- `s(1)` / `s.apply(2)` still emit `invokeinterface scala/collection/SeqOps.apply` (real
  scalac emits `invokevirtual scala/collection/immutable/List.apply`; this difference predates
  `agent/seqfn`). The results are identical at run time, and `java -Xverify:all` passes.

The fixtures for the `agent/nothingcall` slice -- the case where a **call** whose result type
is `Nothing` (`sys.error(...)` / `Predef.???` / a user-written `def die(): Nothing`) appearing
in a `match` / `if` / `try` arm, at the end of a block, as a whole method body, in argument
position, or in an ascription, type-checked fine but produced a `VerifyError` at class load
time -- use the prefix `nc` (`nc_nothing` / `nc_nothing_sys`) and, for the same reason, live in
`crates/cli/tests/nothingcall.rs`. Two causes were stacked on top of each other. First, an
expression of type `Nothing` is treated throughout `jvm_sort` as leaving no value behind, just
like `Unit`, whereas on the JVM a **call** returning `Nothing` does push one real reference
onto the stack (to `scala/runtime/Nothing$`, or whatever primitive descriptor the callee
declares) -- `throw` itself does not have this type, so it was unaffected, and
`case _ => throw new RuntimeException(...)` had always worked. That ghost reference flowed
straight into the join of `match`/`if` arms, into the `try` result slot, and into argument
lists, where it disagreed with the types pushed by the other arms (`Tuple2`, `Int`, etc.),
producing `VerifyError: Inconsistent stackmap frames`. Second, `jvm_desc` (the function that
builds a method's return-type descriptor) collapsed `Nothing` to the same `V` as `Unit`, so a
user-defined method like `def die(): Nothing` got the descriptor `()V`: callers could not pick
up the reference that is actually pushed, and conversely `emit_return` chose `vreturn` (a
no-argument `return`) from the `V`, disagreeing with a descriptor that was supposed to return a
reference and yielding `VerifyError: Operand stack underflow` / `Method expects a return
value`. Checking real scalac 2.13.16's output with `javap -c` (`T1.die()`, the `tableswitch` in
`T1.f(Int)`, `$anonfun$opt$1`, etc.) showed that nsc always follows a call of type `Nothing`
with an `athrow`, making everything after it unreachable (`println(sys.error("x"))` does not
even emit the `invokevirtual println`), keeps `Nothing` as `Lscala/runtime/Nothing$;` as a
method return type as well (never `V`), and uses `areturn` only where that reference is
tail-returned (static forwarders, the body of a by-name `Function0` lambda). The fix has three
parts. `gen_expr` (`crates/backend/src/gen.rs`) was turned into a thin wrapper that always
appends an `athrow` when the expression's type is `Nothing`. The assembler already has a
dead-code mechanism -- bytes emitted after `athrow`/`return`/`goto` are discarded until the
next label (`Assembler::kill` / `drop_dead`; as a comment inherited from the `ab` slice puts
it, the design deliberately avoids teaching "every emitter about reachability") -- so this one
change propagates to all of `match`/`if`/`try` arms, ends of blocks, argument positions and
ascriptions (the hand-written `pop` on the `Predef.???` path would double up with the `athrow`,
so it was removed and kept only in the `is_unit_like` case). The `Nothing` arm of `jvm_desc`
was changed from `V` to `Lscala/runtime/Nothing$;` (matching `jvm_desc_val`, which already had
this representation), and `emit_return` now chooses `areturn` when handed `Nothing`.
`nc_nothing.scala` is written using only `die(): Nothing` and `???`, so it runs under
`java -Xverify:all` in **both the private runtime and `--scala-library`**, whereas
`nc_nothing_sys.scala` is the original reproduction case verbatim (`sys.error` plus a `match`
returning a `Tuple2`, plus a by-name argument to `Option.getOrElse`) and is therefore
library-dual-run only. `nc_nothing_wholly_diverging_methods_end_at_athrow` /
`nc_nothing_diverging_arms_still_grow_an_athrow` /
`nc_nothing_user_method_descriptor_is_not_void` pin down the shape of the bytecode itself with
`javap -c`: a method whose whole body diverges ends in `athrow`; `match`/`if`/`try` arms
contain an `athrow` but end with the `return` on the live side; and `die()`'s descriptor is
`Nothing$` rather than `V`. The existing explicit-`throw` path (`explicitThrowArm`) is included
in the same fixture as a regression check.

#### Remaining

- There is a separate hole on the path that narrows an overload from an actual argument of
  type `Nothing`, as in `println(sys.error("x"))` (`ambiguous overload for println with
  arguments (Nothing)`). That is a typer-side overload resolution matter, unrelated to this
  backend fix, so `nc_nothing_sys.scala` sidesteps it via a single-signature method
  `takeAny(a: Any): Unit`.
- The private runtime's `scala/Tuple2` does not override `toString`, so printing one gives
  `scala.Tuple2@<hash>` (in jar mode, and with real scalac, it is `(1,1)`). This is a
  pre-existing difference unrelated to this fix, so `nc_nothing.scala` does not print the tuple
  directly and compares through `._1` instead.

### cats-effect's summoner (`F.type`) and `$this` interpolation (`agent/cats2`)

The bug: cats-effect type class summoners declared as `def apply[F[_]](implicit F: Async[F]): F.type` were being rejected wholesale by the unpickler, so their erased classfile signature was used instead and the implicit parameter list was never filled in. The second, unrelated root was that `$this` in a string interpolation was being read as an `Ident` and searched for as a term.

The tests are in `crates/cli/tests/cats2.rs`; the fixture prefix is `c2`.

The measurement went from `files=184 errors=155 files_with_errors=52` to
**`files=184 errors=151 files_with_errors=52`** (-4).

The brief's hypothesis was that "member resolution on type projections `A#B` is the root, and
`<notype>` / `Any` cascades from there into cats," but that was **false**. The
`BasicBackend.scala` / `ConcurrencyControl.scala` cluster has nothing to do with type
projections; the two roots are as follows.

#### 1. Summoners whose result type is the `F.type` of their own parameter

The cats-effect type classes write their companion summoners like this:

```scala
object Async {
  def apply[F[_]](implicit F: Async[F]): F.type = F
}
```

That `F.type` is a `SINGLEtype` in the pickle, and what it points at is **the method's own
implicit parameter**. `PickleSupply::conv` could only read module singletons (`p.x.type`), so
`Async$#apply` was **rejected outright** as having an "unmappable result type", and only the
classfile-side reading survived -- `apply(x$0: Async[F]): Async[F]`, built from the erased
descriptor. The JVM has no notion of implicits, so `x$0` is an **explicit** parameter;
`adapt_implicit_apply` did not fill in an implicit list, and `Async[F]` stayed a method type.
The result:

```
error: value flatMap is not a member of (Async[F])Async[F]
error: value pure is not a member of (Sync[G])Sync[G]
```

cats-core writes the same summoner as `: Applicative[F]`, so `Applicative[F]` worked while
`Async[F]` did not -- an asymmetry that was hard to explain until this was found.
(The three `>>` cases that tail4 described as "looking like a cascade because the arguments are
already `Any`/`AnyRef`" are **something else**; they are still at three after this fix.)

The fix is one rule added to `PickleSupply`: a `p.type` that points at a parameter of the
member itself widens to that parameter's declared type (`param_singletons`). The type of the
value that `F.type` denotes is `Async[F]`, so the set of members selectable from the summoner's
result is unchanged.

There was one more thing. On the path by which `import cats.effect.Async` actually works --
the `cats.effect` package object's `val Async = cats.effect.kernel.Async` -- the module class
`Async$` is only stubbed by `find_or_stub_java_class` and is **never adopted from the pickle**,
so `complete_named` never handed out its members in the first place (`value flatMap is not a
member of Async$`). The `Module[T]` -> `Module.apply[T]` redirect now adopts the module class
immediately before demanding `apply` (`Check::adopt_cp_module_class`). It does not adopt
speculatively, because adopting a companion brings in all of its members.

#### 2. `$this` in string interpolation

`this` is a keyword, not an identifier, so `s"for $this"` is the expression `this`, exactly as
`${this}` would be. Because it was read as an `Ident`, it was looked up as a term, and slick's
`s"No type for symbol $sym found in $this"` became `not found: value this` (two cases, in
`Type.scala` and `BasicBackend.scala`).

#### Verification

`c2_thisinterp.scala` passes `-Xverify:all` under both `--scala-library` and
`--no-scala-library`, and its stdout is checked to match real scalac 2.13.16.
`c2_thisinterp_bad.scala` pins down that `$name` has not become a wildcard that accepts
anything (`not found: value nosuchvalue`).

`a_summoner_returning_its_own_parameters_type_crosses_a_jar` compiles a small library that has
both a summoner returning `F.type` and a re-export through a package object with **real
scalac**, packs it into a jar (our own writer does not emit `SINGLEtype`, so the fixture is
meaningless unless it comes from scalac), and runs a program that can only see the jar.
A `TC[Crate]` with no witness still reports
`could not find implicit value of type TC[Crate]`.

Because the seam between the parser and `pickle_supply.rs` was touched, `cargo test
--workspace` was run (in `--release`). The subset is unchanged at
`38 files / 204 classes / verified=204 failed=0`.

#### Known remaining issues

- In the shape `Plain[Box].unit` -- selecting a member that **takes no arguments** from the
  result of a companion summoner, with a concrete class as the type argument -- the class's
  type parameter is not as-seen-from'd and comes back as `F[Unit]`. This does not reproduce
  with real cats (`cats.Applicative[G].unit` works) and does not appear in slick, so it is out
  of scope here.
- Writing `def apply[F[_]](implicit F: TD[F]): F.type` **in source** gives
  `type mismatch; found: F[Int] required: F[Int]`. The fix above is on the pickle path only;
  `F.type` in source goes through a different path.
- If the same file also has a definition that takes an `Async` as an explicit implicit
  parameter, then `Functor[F].map(x)(f)` reports
  `no matching overload for (F[A])((A) => B)F[B]` (this was already the case on `main` before
  the fix). A different hole that depends on the order in which the cats side completes.
- Two cases where the existence of a `slick.cats` package turns `cats.effect.IO` inside
  `slick.dbio` into `value effect is not a member of <notype>`. The root has been identified:
  `Check::expose_unqualified` walks **every** enclosing package up the owner chain. nsc does
  not do that: from a **qualified package clause** `package p.q`, neither `p`'s classes nor its
  subpackages are visible (2.13.16 reports `not found: type Widget` / `not found: value cats`
  with or without `-Xsource:3`), whereas from a nested `package p { package q { ... } }` both
  are. Changing it to walk only the packages that the file's package clause opens (one per
  `PackageDef`) does make those two errors go away, but then the **qualified** reference
  `slick.ControlsConfig` from `package slick.jdbc` stops resolving, for a net +1, so it was
  reverted for now. The rule itself is correct; it needs to be untangled together with the
  other places that lean on the loose reading.
  -> Resolved in `agent/proj` (the section "Re-reading members of a type projection `A#B`, and
  what a `package` clause opens"). What was leaning on it was **default argument right-hand
  sides being typed in the caller's scope**.
- tail4's leftover `value database is not a member of BasicBackend.Session` (re-reading members
  of a type projection) is also untouched.
  -> Resolved in `agent/proj`. tail4's diagnosis was correct.
- The three cats `>>` cases (`no matching overload for (=> F[B])(FlatMap[F])F[B]`) are at three
  both before and after. They are not caused by `Async` / `Deferred` collapsing; there is a
  separate reason the left-hand side of `decrementDepth >> releaseIfUnpinned >> ...` falls to
  `Any` / `AnyRef` (`BasicBackend.scala` went 6 -> 5, `ConcurrencyControl.scala` also 6 -> 5).

### Eight roots behind the 11 remaining `type mismatch` errors in slick (`agent/mismatch13`)

The bug: 11 `type mismatch` errors were left in slick, looking like eleven separate inference failures. They turned out to have eight distinct roots, most of them small contract violations -- applying a substitution more than once, missing the head of a base type sequence, or re-substituting with the receiver's own type arguments.

The tests are in `crates/cli/tests/mismatch13.rs`; the fixture prefix is `mism13`.

The measurement went from `files=184 errors=155 files_with_errors=52` to
**`files=184 errors=141 files_with_errors=48`** (-14 errors / -4 files).
`tests/slick_subset.sh` is unchanged at `38 files / 204 classes / verified=204 failed=0`.
`type mismatch` went from **11 to 2**, and both remaining ones are cascades from errors other
than `type mismatch` (see "What is left" at the end).

| cluster | before | after |
|---|---|---|
| `found: Tuple2[T, T2] required: (((T, T2), T2), T2)` and other `ShapedValue.zip` cases | 2 | **0** |
| `found: DBIOAction[R, S, E with Effect] required: DBIOAction[Any, NoStream, Effect]` and others | 2 | **0** |
| `found: P required: Rep[Option[QO]]` (`ExtensionMethods.flatten`) | 1 | **0** |
| `found: Product required: Option[Option[Any]]` (`SQLiteProfile`) | 1 | **0** |
| `found: Query[G, T, U] required: Query[G, T, C]` (`Query.zipWith`) | 1 | **0** |
| `found: State[_] required: State[F]` (`ConcurrencyControl.create`) | 1 | **0** |
| `found: <overload String \| <error>> required: String` (`Node.toString`) | 1 | **0** |
| `not found: type DumpInfo` / `no matching overload for (...)DumpInfo` | 3 | **0** |
| `no implicit: could not find implicit value of type <:<[...]` | 1 | **0** |
| `not found: type Mapper` | 1 | **0** |

Only one of the inherited diagnoses could be confirmed. `tail4`'s note that "`lub` does not
build intersection types, so one `found: Product required: Option[Option[Any]]` remains" had
**the right location but the wrong reason**: no intersection type is needed; `lub` simply was
not looking at the **head** of the base type sequence (i.e. the type itself) (see 4).
`JdbcActionComponent`'s `E with Effect` was not an intersection-type problem either but a
**type variable inside a lambda's result type** (see 3), and `Query.scala` /
`RelationalProfile.scala` / `Node.scala:636` / `ConcurrencyControl.scala:202` each had a
different root.

#### 1. The substitution was applied three times (when `new C[...]` is the enclosing class itself)

`pick_ctor_at` (`crates/typer/src/check.rs`) applied `subst_tparams(class_id, targs, ...)`
once via `flatten` to test applicability, and once more to the result that came back from
`resolve_overload`. The `new` side then applied it a third time with
`p = subst_tparams(c, &inferred_args, &p)`. As long as the type arguments do **not contain the
very type parameters being substituted**, this is idempotent, which is why nobody had noticed.
`new ShapedValue[(T, T2), (U, U2)](...)` inside `ShapedValue[T, U]` is exactly the case where
they do: `T` becomes `(T, T2)` -> `((T, T2), T2)` -> `(((T, T2), T2), T2)`, giving
`found: Tuple2[T, T2]  required: (((T, T2), T2), T2)`.

`pick_ctor_at`'s contract is now fixed as "the argument types and result type it returns are
read with `targs`, exactly once." When there is a single candidate, `flatten`'s result is
returned as-is so no substitution happens on the way out; substitution happens only when there
are two or more candidates (because `resolve_overload` re-reads candidates from the symbol only
for `Type::Overload`). The callers on the `extends` side and the `new` side dropped their
corresponding re-substitutions.

#### 2. `<:<` is an implicit **view** (in both the typer and codegen)

nsc asks whether the candidate's type conforms to `From => To`, which means a value of a class
that **inherits** `Function1` is a view too. `scala.<:<` is precisely
`sealed abstract class <:<[-From, +To] extends (From => To)`, and slick's

```scala
def flatten[QO](implicit ev: P <:< Rep[Option[QO]]): Rep[Option[QO]] =
  flatMap[QO](identity(_))
```

(`lifted/ExtensionMethods.scala:210`) relies on nothing else.
`conversion_provides` (`crates/typer/src/implicits.rs`) only looked at structural
`Type::Function`s and single-argument methods, so neither `r: P` inside `Ext` nor the result of
`identity(_)` could become a `Rep[Option[QO]]`. `view_shape` was factored out; for class types
it now picks the `FunctionN` shape out of the base type sequence. Implicit methods that
**take no arguments** are not treated as views (`<:<.refl[A]: A =:= A` would otherwise convert
every type to itself).

There was a hole on the codegen side too. Applying a view produces the tree
`Apply { fun: <reference to ev>, args: [x] }`, but `gen_apply`
(`crates/backend/src/gen.rs`) only emitted `FunctionN.apply` when `fun.ty` was a structural
`Type::Function`, falling back to `invoke_method(fun.sym)` otherwise. `ev` is a **value**, not
a method, so what came out was a member call on the enclosing **method**, producing
`NoClassDefFoundError: direct` (after type checking had passed). It now also routes to
`gen_function_apply` when `fun.sym` is not a method, its type's class inherits `FunctionN`, and
the arity matches.

#### 3. Type variables that appear only inside a lambda's **result**

The `B` in `def h[B](f: Int => Bx[B]): Bx[B]` can only be determined by the lambda body. When
`p` was exactly `Type::Function { ret: TypeParam }` it was already relaxed to `Any`, but one
level in, as in `Bx[B]`, `open_to_bounds` opened it to the bound and produced `Bx[Any]`, which
the invariant `Bx[Int]` does not conform to -- the argument was dropped before the second
inference pass could read `B` off the body.

The fix relaxes **only the expected type** used when typing the argument. It installs a
`Type::Wildcard` (a form `is_sub_type` can treat as "not yet determined"; `open_to_bounds`
already uses it for higher-kinded parameters) rather than the bound, so the body still learns
that it has to be a `Bx`. `p` itself stays as declared, so `solve_open_from_arg` reads `B` off
the already-typed argument. A wildcard left in the lambda's type gets carried all the way into
the call's result (`Act[_, _, Effect with _]`), so the existing cleanup that re-stamps things
with the body's type was extended to cover cases containing wildcards.
slick's `DBIOAction.flatMap[R2, S2, E2](f: R => DBIOAction[R2, S2, E2])` is this shape.

#### 4. `lub` was not looking at the **head** of the base type sequence

`agent/tail4` made `lub` "join the type arguments and stop when both sequences meet at the same
class," but `base_type_seq` does not return **the type itself** (in SLS 3.5.2 it is at the
head). `lub(Some[X], Option[Y])` therefore could not find `Option` in the second sequence,
walked straight past `Option[X]`, and landed on `Option`'s own parent `Product`. Simply adding
the type at the head of both sequences yields `Option[Option[Any]]`. `tail4`'s note said the
reason was "because it does not build intersection types," but
`Option[X] with Product with Serializable` is not needed.

#### 5. Inherited members are read with the type parameters of the class that **declared** them

`type_select` (`crates/typer/src/check.rs`) walked the parents with `subst_as_seen_from` and
read the member correctly -- and then applied `subst_tparams(owner, recv_args, ...)` a second
time with the **receiver's own type arguments**. The positions only line up when the receiver
is the declaring class itself. In slick's
`BaseJoinQuery[E1, E2, U1, U2, C, B1, B2] <: Query[+E, U, C[_]]`, the join's first three
arguments got substituted into `Query`'s three parameters, so `Query.map`'s `Query[G, T, C]`
became `Query[G, T, U1]` (`Query.zipWith`). What is more, the second application only has an
effect **when the first one was the identity** -- `stdJoin` writes the enclosing class's own
`C`, so `C := C` -- which is why writing the same shape in a small test does not reproduce it.
The second substitution was dropped for class-type receivers (tuples and function types keep
positional substitution, because `subst_as_seen_from` cannot walk them). The `extends` and
`new` sides were treated the same way.

#### 6. Explicit type arguments are the **expected type** for that argument

`proto_arg_type` only built an expected type when the parameter was exactly a bare type
parameter. In `Ref.of[F, State[F]](State(max, min, TreeMap.empty))`
(`basic/ConcurrencyControl.scala:202`), `[F, State[F]]` is written out explicitly, so the
parameter is already `State[F]`. Without passing that down, `State(...)` was typed with no
expected type, and the **higher-kinded `F`** of `case class State[F[_]]` -- which appears in no
argument -- was undetermined, giving `State[_]`. Parameters that are already fully determined
by the type arguments are now passed straight through as the expected type.

#### 7. The target of a `copy` rewrite was spelled by **name**

`copy(x = 1)` is rewritten to `{ val t = recv; new C(t.a, 1, ...) }`, but the `C` in that
`new C` was built as an `Ident` **by name**, so it gets resolved in the scope of the file where
the rewrite ran. slick's

```scala
override def getDumpInfo = super.getDumpInfo.copy(mainInfo = s"idx=$index")
```

(`jdbc/JdbcResultConverter.scala` / `memory/MemoryQueryingProfile.scala`) only knows `DumpInfo`
through an inherited member and never imports it, so it produced a **position-less**
`not found: type DumpInfo`. The fix uses the "already-resolved type" marker that
`crate::materialize` already has (nsc's `TypeTree(tp)`) to put the symbol itself in place.
The three files `tests/multi/mism13_*.scala` reproduce this.

#### 8. Joining `if` / `match` branches

`Node.getDumpInfo` (`ast/Node.scala`):

```scala
val ch = this match {
  case Path(_ :: _ :: _) if !GlobalConfig.dumpPaths => Vector.empty
  case _                                            => childNames.zip(children.toSeq).toVector
}
```

This is a join of `Vector[A]` (with `Vector.empty`'s undetermined `A`) and
`Vector[(String, Node)]`; the argument join walked all the way to `AnyRef`, giving
`Vector[AnyRef]`, so `DumpInfo(..., ch)` did not type-check, which made `getDumpInfo`'s
inferred type an error, which made `override final def toString` an error, and finally
`n.toString` reported `found: <overload String | <error>>  required: String`
(`Node.scala:636`) -- a four-stage cascade.

nsc's `solve` reads any variable that nothing constrained at its bound, so `Vector[Nothing]`
is a `Vector[(String, Node)]`. `lub_branches` now does the same before joining, but with three
conditions attached so that only the misses are closed: the type parameter must **not be
reachable by name from this scope** (the `T` of an enclosing `def f[T]` is reachable, so it
stays open), the other branch must not mention it, and it must be in a covariant position. On
top of that, **the answer is always one of the two branch types** (the closed result is
returned only when it turns out to be a subtype of the other branch; otherwise the usual `lub`
applies). There is a separate hole where `Option.getOrElse`'s `[B >: A]` cannot be read from
the pickle, and the more accurate join brought it to the surface; this last condition keeps it
suppressed.

#### Verification

`mism13_lang.scala` passes `-Xverify:all` under both `--scala-library` and
`--no-scala-library`, and is also cross-checked against real scalac 2.13.16's stdout
(`expected/mism13_lang.txt`). `mism13_lib.scala` is library-mode only, since `<:<` only exists
on the jar side; in the private runtime it reports `not found: type <:<`. `mism13_bad.scala`
rejects 6 cases, and nsc 2.13.16 reports the same 6. 10 of the 13 tests were confirmed to
**fail on `main` before the fix**. `mismatch13` / `mismatch12` / `tail4` / `buildfrom2` /
`conform` / `e2e` / `multifile` and `cargo test --workspace` were run in `--release`.
`cargo clippy --workspace --all-targets` still reports 70 warnings (only line numbers shift);
none are new.

**What is left** (not fixed in this slice):

* `found: Some[Tuple2[TableNode, ConstArray[T]]]` at
  `slick/compiler/MergeToComprehensions.scala:218`. The root is three lines above, where
  `tableFields.getOrElse(t.identity, Seq.empty)` produces
  `no matching overload for (Any, => Vector[TermSymbol])Vector[TermSymbol]`.
  `prelude_coll.rs` models `Map.getOrElse[V1 >: V](key: K, default: => V1): V1`
  monomorphically at `V`, so it cannot accept `Seq.empty`. Five lines reproduce it:
  `val m: Map[String, Vector[Int]] = Map.empty; m.getOrElse("k", Seq.empty)`.
* `found: C required: CompiledFunction[...]` at
  `slick/relational/RelationalProfile.scala:72`. Downstream of
  `no implicit: could not find implicit value of type TypedType[Boolean]` on the same line
  (`Library.==.column[Boolean](...)`): the `C` in
  `Compiled.apply[V, C <: Compiled[V]](raw: V)(implicit compilable: Compilable[V, C], ...): C`
  can only be determined by the witness. The implicit side has to come first.
* `Option.getOrElse[B >: A](default: => B): B`'s `B` cannot be read from the pickle (the
  signature comes out as `(=> A)A`, and `Option(1).getOrElse("x")` gives
  `no matching overload`). The same shape written in source
  (`def orElseN[B >: A](d: => B): B`) works, so the hole is on the unpickler side.
* Calling `Ext[P].flatten` **without type arguments** cannot solve `QO` from the `<:<` witness
  (`e.flatten` fails; `e.flatten[Int]` works). This is a hole in `implicit_solve` for the case
  where the caller's type parameter is **nested**, as in `Rp[Option[QO]]`; slick itself does
  not call it in this shape.

### 14 of the 49 `no matching overload` errors in slick (`agent/ovl3`)

The bug: 49 `no matching overload` errors in slick. The root insight is that this message is not only about ambiguity -- it also fires when there is a single candidate that does not accept the arguments, which means signatures the prelude had modelled monomorphically were masquerading as "missing overloads."

The tests are in `crates/cli/tests/ovl3.rs`; the fixture prefix is `o3`.

The measurement went from `files=184 errors=134 files_with_errors=48` to
**`files=184 errors=120 files_with_errors=41`** (-14 errors / -7 files).
`no matching overload` went from **49 to 35**.
`tests/slick_subset.sh` is unchanged at `38 files / 204 classes / verified=204 failed=0`
(`crates/backend` was not touched, so that number was measured just once, before adding the
`StringBuilder` constructor).

`no matching overload` is not a message about "several candidates and no way to choose."
**Even with only one candidate**, the same sentence is printed if that one candidate does not
accept the arguments. In other words, what this cluster really was is *signatures the prelude
modelled monomorphically* looking like "missing overloads." The 49 surface errors reduce to
5 roots (one of which was simply a missing constructor).

| root | before | after |
|---|---|---|
| missing `[B >: A]` on `Option.getOrElse` / `orElse` / `Map.getOrElse` | 7 | **0** |
| `mutable.HashSet` / `HashMap` were not `collection.Set` / `Map` | 4 | **0** |
| a view that only exists in the pickle (`Option.option2Iterable`) was not being read | 1 | **0** |
| prelude and pickle declarations with identical signatures came out `ambiguous` | 0 | **0** (resolves what appeared as a side effect of 2) |
| `new StringBuilder(Int, String)` was missing from the prelude | 1 | **0** |

#### 1. The missing `[B >: A]` (`crates/typer/src/prelude_ovl3.rs`)

nsc has `def getOrElse[B >: A](default: => B): B`. The prelude had `(=> A)A` in
`prelude_either.rs`, and in `prelude_coll.rs` it modelled `getOrElse[V1 >: V]`
"monomorphically at `V`" (the comment says so explicitly). So
`(o: Option[Sub]).getOrElse(base)` reported
`no matching overload for (=> Sub)Sub with arguments (Base)`.

`Typer::infer_method_tparams_in` already takes the lub of the type solved from the arguments
and the lower bound (`prelude_lowbound.rs` uses this for `List.::`), so **declaring the lower
bound** is the entire fix. `B` / `V1` are type parameters, so erasure is unchanged and neither
the private runtime's ABI nor the real jar's is affected. The methods covered are
`Option.getOrElse` / `Option.orElse` / `immutable.Map.getOrElse` / `mutable.Map.getOrElse`.

The seven affected sites in slick are `EmulateOuterJoins.scala:78`, `CreateAggregates.scala:54`,
`MergeToComprehensions.scala:215`, `H2Profile.scala:71`, `MySQLProfile.scala:94`,
`SQLServerProfile.scala:112` and `JdbcModelBuilder.scala:253` (the
`m.getOrElse("k", Seq.empty)` that `mismatch13` listed as remaining is this too).

#### 2. Parents of `mutable.HashSet` / `HashMap` (`prelude_ovl3::install_hierarchy`)

The edge table in `prelude_hier.rs` had `mutable/Set` -> `collection/Set` but not
`mutable/HashSet` -> `mutable/Set`, because `add_hash_set` / `add_hash_map` (prelude.rs) still
built them with `&[Type::AnyRef]`. slick passes `mutable.HashSet.empty[TypeSymbol]` to
`def containsSymbol(tss: scala.collection.Set[TypeSymbol])`, so `Util.scala:72` /
`ExpandSums.scala:323` / `ExpandTables.scala:73` / `ExpandTables.scala:82` were failing.
`LinkedHashSet` / `LinkedHashMap` have the same shape and were included as well.

#### 3. Duplicates with identical signatures (`resolve_overload`, `crates/typer/src/check.rs`)

As a side effect of adding the edges in 2, `mutable.HashMap` started seeing `getOrElse` by
**two routes** -- the prelude's `mutable.Map` declaration, and the pickle declaration of
`collection.MapOps` imported from the jar. Both are `(K, => V1)V1`, and in nsc they would be a
single symbol. `resolve_overload` already had a "same signature means one candidate" rule,
`winners.dedup_by(|a, b| a.1 == b.1 && a.2 == b.2)`, but the two `V1`s are **different
symbols**, so `==` did not hold. The comparison now renames one side's type parameters to the
other's before comparing (`canonical_sig`), which makes it do what it was meant to do. The
candidate that came first (i.e. the one closer to the receiver) is kept, so the same side is
chosen as before the edges were added.

#### 4. When to read a view that exists only in the pickle (`check.rs`)

`Seq("a") ++ anOption` needs `option2Iterable`. That implicit is not in the prelude;
`warm_pickled_implicits` supplies it from the pickle (its comment names
`Option.option2Iterable` explicitly). But the applicability test (`arg_conforms` ->
`search_conversion`) runs on `&self` and therefore **cannot read classfiles**. The result was
unreproducible behaviour: it only worked when an earlier line in the same file happened to
select an `Option` member and warm it up (via `search_extension`).

Now, when `resolve_overload` returns `None` and when `adapt` descends into conversion search --
both places where failure is already established -- the companion of **the type itself** is
warmed and the question is asked once more. This happens at most once per class, so the cost is
bounded.

Not warming base classes is deliberate. Reading a companion's pickle installs that companion's
pickled parents, and for collections those turn out to be factory types that the **prelude
hand-writes**, such as `IterableFactory.Delegate`. Warming the whole implementation-type scope
of `mutable.Set[T]` attached `Delegate` to `Iterable$` / `Seq$` / `Set$`, and its
`apply[A](A*): CC[A]` lined up alongside the prelude's `apply`, turning `mutable.Set[TypeSymbol]()`
into `Set[A]` (this regression was actually observed, hence stopping at the type's own class).

#### 5. `new StringBuilder(initCapacity, initValue)`

The constructor table in `prelude_text.rs` had only `()` / `(Int)` / `(String)`, so
`new StringBuilder(s.length, "")` at `TableDump.scala:50` was failing. This is
`library_abi`-only -- under `--no-scala-library`, `scala.collection.mutable.StringBuilder`
falls back to `java.lang.StringBuilder`, which has no `(int, String)` constructor to begin
with.

#### Verification

`o3.scala` passes `-Xverify:all` under both `--scala-library` and `--no-scala-library`, and is
cross-checked against real scalac 2.13.16's stdout (`expected/o3.txt`). `o3_lib.scala` is
library-mode only, since the `mutable.HashSet` / `collection.Set` members exist only on the jar
side; in the private runtime it reports `value size is not a member of Set[String]`
(`expected/o3_lib.txt`). Both of them actually **run** `mutable.HashMap`'s `getOrElse`, so if
the deduplication in 3 had picked a different symbol and broken something, either
`-Xverify:all` or the output comparison would fail. `o3_bad.scala` checks that
`Option[Int].getOrElse("no")` comes out as `Any` (it widens to the lub, and does not become
`Int`). nsc 2.13.16 rejects the same line. 6 of the 7 tests were confirmed to **fail on `main`
before the fix** (the remaining one is a negative test checking a diagnostic under
`--no-scala-library`, where compilation fails before the fix as well).
`overloadshadow` / `ambigmap` / `setapply` / `uniteq` / `integral` / `ordsummon` / `mutcoll` /
`conform` / `ovl2` / `ovl3` / `mismatch13` / `buildfrom2` / `lowbound` / `e2e` and
`cargo test --workspace` were run in `--release`. All 78 `cargo clippy --workspace
--all-targets` warnings are in places this slice did not touch; none are new.

**What is left** (35 `no matching overload` errors, not fixed in this slice):

* `java.util.Arrays.copyOf[Any](a: Array[AnyRef], n)` (`ConstArray.scala:314` / `516`, 2
  cases). nsc reads `Object` in a Java signature as `ObjectTpeJava` and conforms it to both
  `Any` and `AnyRef`, so **the call itself succeeds** and only the assignment of the result to
  an `Array[Any]` reports `found: Array[Any] required: Array[Any]` (confirmed with scalac).
  Here it fails outright on `Array`'s invariance.
* The implicit conversion `Array[T]` -> `IterableOnce[T]` (`Predef.genericWrapArray` /
  `wrapRefArray`) is not registered as a view. Three shapes: `Map() ++ anArray`
  (`JdbcTypesComponent.scala:526`), `TupleSupport.buildTuple(anArray)`
  (`ResultConverter.scala:58`), and `val xs: IndexedSeq[Any] = anArray`. The backend side
  already has `emit_array_wrap_to_iterable_ops`.
* `Set() ++ anOption` (`JdbcModelBuilder.scala:280`). A continuation of 1: `Set.++` / `Seq.++`
  still have their `[B >: A]` modelled monomorphically. This meshes with `prelude_buildfrom`,
  so it belongs in a separate slice.
* `RefId[E <: AnyRef]` is invariant, so `errors.contains(RefId(n1))` needs to **determine
  `RefId.apply`'s `E` from the expected type** (`VerifyTypes.scala:38` / `41`). That means
  reworking the order in which arguments are typed without an expected type before an overload
  is chosen; the blast radius is large, so it was left alone.
* `allTSyms -- referenced.map(_._1)` (`PruneProjections.scala:14`). The type parameters of the
  `map` loaded from the pickle onto the `immutable.HashSet` returned by `.toSet` cannot be
  solved, so the argument stays a `HashSet[A]`.
* `ConfigFactory`'s `c.root.asScala` (`GlobalConfig.scala:71` / `78`, 2 cases). The type
  arguments of the `java.util.Map<String, ConfigValue>` that `ConfigObject` implements cannot
  be read, so it comes out as `Map[AnyRef, AnyRef]`.
* `expansions(tsym)` / `expansions contains tsym` (`ExpandTables.scala:25`).
  `scala.collection.Map` is a **member-less** stub created by `prelude_hier`'s LINKS, so
  `apply` / `contains` depend entirely on the jar's pickle.
* The three cats `>>` cases (`BasicBackend.scala:329` / `432` / `434`) and the three cases in
  `DBIOAction.scala` that pass a `<:<` as a `Function1` have not been investigated.

### Function literals in argument position, base types, and Java's `Object` (`agent/mismatch14`)

The bug: nine errors in slick spread across four apparently unrelated shapes -- function literals in argument position with no expected type, conversions whose type arguments resolved to `AnyRef`, `Arrays.copyOf[Any]`, and abstract type members in inherited result types. The common thread is that each was a place where a type that was already determined somewhere was simply not being handed to the code that needed it.

The tests are in `crates/cli/tests/mismatch14.rs`; the fixture prefix is `mism14`.

The measurement went from `files=184 errors=115 files_with_errors=41` to
**`files=184 errors=106 files_with_errors=41`** (-9).

The two cases `agent/ovl3` left behind with reasons (2 x `Arrays.copyOf[Any]`, 2 x
`ConfigObject.asScala`), plus the 2 `Node.Self` `type mismatch` cases and the 2
`(Statement) => Unit` cases in `JdbcBackend` (plus 1 `missing parameter type for expanded
function` dragged along with them), all reduce to the following 4 roots. Apart from the 9 that
disappeared, **no new diagnostics appeared** (the only difference is the numbering of anonymous
classes).

| root | before | after |
|---|---|---|
| a monomorphic callee was not passing an expected type to its arguments | 3 | **0** |
| conversion type arguments were not solved from the receiver's **base type** | 2 | **0** |
| a Java type argument written as `Any` was substituted as `scala.Any` | 2 | **0** |
| abstract type members in an inherited result type were not re-read in the subclass | 2 | **0** |

#### 1. The expected type was not reaching function literals in argument position

```scala
def take(f: Statement => Unit): Int = 1
take(if (cond) { s => si(s) } else { s => si(s); si(s) })
```

The `s` on the `else` side stayed `<notype>` while calling `si(s)`, producing two
`no matching overload for (Statement)Unit with arguments (<notype>)` errors (one per statement
in the body). That the `then` side worked was **coincidence**: `section_param_types` has a rule
that says "if the body is a single call, pick up the parameter type from that callee's
signature." A two-statement body has nothing to pick up from.

The real culprit was the check at the top of `Typer::proto_arg_type` that said "do not produce
a prototype for a callee with no type parameters." nsc types every argument against its
parameter type (`Typers.typedArg`). Copying that wholesale would have had wide fallout, so it
was restricted to parameters that are **a function type, a `FunctionN`, or a SAM, and that
contain neither type parameters nor wildcards**. That an unsolved type parameter must not be
used as a prototype is exactly what the comment on `agreed_lambda_params` records, with
measurements (the case where fixing cats' `uncancelable[A]` first sent slick from 155 to 232).

The same hole existed on two other paths.

* **Overloads** (`Type::Overload`): a prototype is produced only when every candidate demands
  the same function-type parameter (`agreed_function_param`).
* **Constructors** (`new C(...)`, and the cases that route through `C(...)`): the field types
  are used as prototypes only when the class has no type parameters and the arity matches the
  primary constructor.
* **Companion `apply`**: `rewrite_receiver_apply` does **not** rewrite `Obj(args)` into
  `Obj.apply(args)` (for codegen reasons; see the comment on `named_arg_param_ids`). So the
  callee's type is `Type::ModuleRef` and the parameters live on its `apply`. Forgetting that it
  **inherits** `AbstractFunctionN.apply` here makes the candidates
  `(String, (Statement) => Unit, Int)SP` and `(T1, T2, T3)R`, which never reach "all candidates
  agree," so they are compared after being run through as-seen-from across the module class.
  slick's `JdbcBackend.StatementParameters(..., if (...) ... else { s => ...; ... }, ...)` is
  exactly this shape.

A prototype is a **hint, not a constraint**. As the method path already does, if an argument
typed with a prototype complains (or if the result does not conform to the parameter), the
diagnostics are thrown away along with it and the argument is re-typed without the prototype.
Forgetting to add this rollback on the constructor path made slick's
`new StructValue(..., xs.toMap)` newly fail: `StructValue`'s second parameter is
`TermSymbol => Int`, and there is no path that solves `toMap`'s `K` / `V` through
`Map <: Function1`. Typed without a prototype it is `Map[TermSymbol, Int]`, which conforms
directly.

#### 2. Conversion type arguments are solved from the receiver's base type

`Typer::conv_targs` matched the conversion's parameter against the receiver **positionally**.
For `java.util.Map[K, V]` with a receiver of `ConfigObject` (which has no type arguments at
all) there is nothing to zip against, so both `K` and `V` fall to `AnyRef`. It now converts to
`java.util.Map[String, ConfigValue]` with `base_type_instance` first and solves from that. The
same thing happened in pure Scala (with `class Sub extends Base[String, Int]`, the
`sub.firstOf` of an `implicit class Ops[A, B](b: Base[A, B])` came out as `AnyRef`).

#### 3. An `Any` written for a Java type parameter is `Object`

nsc reads `Object` in a Java signature as `ObjectTpeJava`. Calling `<T> T[] copyOf(T[], int)`
as `copyOf[Any](...)` pins `T` to `Object` rather than `scala.Any`, so `Array[AnyRef]` is
accepted and only the assignment of the result to an `Array[Any]` fails (confirmed with real
scalac: the seemingly nonsensical message `found: Array[Any] required: Array[Any]` occurs
because the `found` side is really `Array[Object]`). scala-rs now re-reads an `Any` written
explicitly in a `TypeApply` as `AnyRef`, but only for type parameters bounded by `Object` on a
callee with the `JAVA` flag set (`java_object_targs`). `Array`'s invariance is unchanged, so
`copyOf[Any](Array[String], 3)` is still rejected (`mism14_bad.scala`).

#### 4. Abstract type members in an inherited result type

```scala
trait Node { type Self >: this.type <: Node; def rebuild(ch: …): Self }
case class StructNode(…) extends Node {
  type Self = StructNode
  override def rebuild(ch: …) = StructNode(…)   // found: StructNode required: Node.Self
}
```

For an override with no written result type, `overridden_ret_type` takes the type from the
parent's declaration, but `subst_as_seen_from` only substitutes type **parameters** -- abstract
type **members** were left as they were. nsc sees the declaration from
`StructNode.this.type`, so `Self` is `StructNode`. The type members of the result type that was
taken from the parent are now replaced with the concrete aliases the subclass itself has under
those names (`own_type_members`). slick's `StructNode` / `Filter` in `ast/Node.scala` are this.

#### Verification

`mism14.scala` passes `-Xverify:all` under both `--scala-library` and `--no-scala-library`, and
is cross-checked against real scalac 2.13.16's stdout. `mism14_lib.scala` is library-mode only,
since `scala.jdk.CollectionConverters` exists only on the jar side; in the private runtime it
reports `value asScala is not a member of Names`. `mism14_bad.scala` pins down, message text and
all, that `Array`'s invariance still holds (before the fix the message was
`(Array[Any], Int)Array[Any]`, so this negative test also fails on `main` before the fix). All
7 tests were confirmed to fail on `main` before the fix. In addition to
`cargo test --workspace`, the following were run in `--release`: `overloadshadow` / `ambigmap` /
`setapply` / `uniteq` / `integral` / `ordsummon` / `mutcoll` / `conform` / `e2e` / `mismatch14` /
`ovl3` / `mismatch13` / `buildfrom2`.

**What is left** (not fixed in this slice):

* The `missing parameter type for expanded function` at `RelationalProfile.scala:82` is a
  separate matter (it is on the `mp.genericFastPath { ... }` side).
* Of the remaining items listed by `agent/ovl3`, the `Array[T]` -> `IterableOnce[T]` view,
  `Set.++` / `Seq.++`'s `[B >: A]`, solving `RefId[E <: AnyRef]` from the expected type, the
  member-less `collection.Map` stub, and cats' `>>` are all untouched.

### Re-reading members of a type projection `A#B`, and what a `package` clause opens (`agent/proj`)

The bug: two unrelated resolution failures. Members selected through a type projection `A#B` were read from `B`'s owner's declaration rather than through the prefix `A`, and unqualified names were being searched up the entire enclosing-owner chain, so slick's own `slick.cats` package shadowed the real `cats`. Both prior slices had correctly diagnosed these; what was wrong was the *reason* the second fix had previously appeared to regress.

The tests are in `crates/cli/tests/proj.rs`; the fixture prefix is `pj`.

The measurement went from `files=184 errors=134 files_with_errors=48` to
**`files=184 errors=129 files_with_errors=48`** (-5).
`tests/slick_subset.sh` (at the start: `38 files / 204 classes / verified=204 failed=0`) was
**not run**: this slice is on the typer side, and the only backend change is one place in
`pickle.rs` (writing a projection's as-seen-from view as a plain parent). The subset's
verification is bytecode verification via `Class.forName(initialize=false)`, which does not read
`ScalaSignature` and therefore cannot detect pickle changes. Instead, as described under
"Verification" below, `jarpickle` / `e2e` / `multifile` were run, and the pickle-side effect was
observed directly as a byte-level classfile difference.

`agent/tail4`'s diagnosis ("the `session` of `HeapBackend#BasicActionContext` is not being
re-read with the projection target's prefix") was **correct**. `agent/cats2`'s diagnosis ("the
root of `value effect is not a member of <notype>` is that `expose_unqualified` walks every
enclosing package up the owner chain") was **also correct**. What was wrong was the **reason**
given for "adding that rule stops `slick.ControlsConfig` from resolving" -- it turned out to
have nothing to do with package resolution (see 2 below).

#### 1. `A#B` was dropping the prefix

`project_from_prefix` (`crates/typer/src/check.rs`) answers a projection with a plain
`Type::Class`. There is nowhere in `Type::Class` to write a prefix, so the moment `A#B` was
built, the fact that it is "being seen through `A`" was lost, and subsequent selections read
members from **`B`'s owner's declaration**. In slick's

```scala
def run(ctx: HeapBackend#BasicActionContext): R = f(ctx.session)
```

it is `BasicBackend` that declares `session: Session`, and there
`type Session >: Null <: BasicSessionDef` is abstract. It is `HeapBackend` that says
`type Session = HeapSessionDef`, so the result was
`value database is not a member of BasicBackend.Session`.

The fix is to have the projection carry along **just what the prefix determines** as a
type-only refinement (`Checker::projected_class_type` / `projection_refinements`). It collects
the names of type members that `B`'s **lexically** enclosing class (and its ancestors) leave
abstract, and if the prefix's class gives them a concrete definition, attaches them as
`type S = Sess`. Member lookup already reads refinements in `expand_in_type` /
`subst_as_seen_from`, so `ctx.session` becomes `Sess`.

**But a refinement is also a constraint.** The first version attached it as-is, which broke
passing a plain `JdbcSessionDef` obtained through the alias `type Session = JdbcSessionDef` to a
parameter of type `JdbcBackend#JdbcSessionDef` -- something slick does everywhere -- producing
**8 new errors** in `JdbcActionComponent` / `JdbcProfile` / `StreamingInvokerAction`
(`no matching overload for (JdbcSessionDef { type Database[_] = ...; type Session = ... })R with
arguments (JdbcSessionDef)`). That is 134 -> 138.

So the refinement gets one extra decl, `symbol::AS_SEEN_FROM_MARK` (`<asSeenFrom>`, a name that
can never be a Scala identifier), as a **mark**, and `SymbolTable::is_sub_type` and
`display_type` read a marked refinement as a plain parent. This encodes the distinction that
as-seen-from is about "how it looks," not "what it demands." A type-only refinement is erased to
its parent, so the classfile side is still a plain `B`. The same holds for the pickle:
`backend/src/pickle.rs` writes a marked refinement as a plain parent (so that the name
`<asSeenFrom>` never appears in a signature). For `A#B#C` the mark is stripped and the
projection is redone from the parent, carrying forward whatever is determined.

#### 2. `package p.q` does not open `p`

`expose_unqualified` searched for unresolved names **by walking up the owner chain**. nsc does
not do that (measured on 2.13.16, with or without `-Xsource:3`):

| spelling | `p`'s classes | `p`'s subpackages |
|---|---|---|
| `package p.q` (qualified) | not visible | not visible |
| `package p { package q { … } }` (nested) | visible | visible (shadows a top-level name of the same name) |

slick has its own `slick.cats` package, so under the loose reading `cats` resolved to
`slick.cats` from every file under `package slick.*`, and `cats.effect.IO` in
`slick/dbio/DBIOAction.scala` became `value effect is not a member of <notype>` (2 cases).

`Checker::open_packages` returns the packages opened by the `package` clause of the file being
typed (the namer records one per `PackageDef`) from the inside out, and **returns the root
last**. Keeping the root in the walk is the key point: in the qualified reference
`slick.ControlsConfig` from `package slick.jdbc`, the leading `slick` resolves as a member of the
root. `agent/cats2` had dropped the root along with everything else here, which is what produced
the net +1.

##### The final fallback that was kept, and the real reason for it

With only the correct rule in place, `slick/jdbc/DatabaseConfig.scala` reports
`not found: value ControlsConfig`. The cause was pinned down from the backtrace of a trap set in
`not_found_error`:

```
Typer::not_found_error ← type_expr ← type_apply_in ← type_apply
  ← type_expr ← fill_defaults_and_implicits ← type_apply_in ← …
```

**Default argument right-hand sides are being typed in the caller's scope.** When
`default_getter_apply` cannot find the `f$default$n` getter, both `fill_defaults_and_implicits`
(via `type_default_rhs_here`) and the named-argument path splice the saved **untyped tree**
straight into the arguments, and it gets typed at the call site.
`slick/basic/DatabaseConfig.scala` writes `import slick.{ControlsConfig, SlickException}` and
`classLoader: ClassLoader = ClassLoaderUtil.defaultClassLoader`, and the caller in
`package slick.jdbc` does not have that import. The loose package walk was merely papering over
it (the same hole shows its face on pre-fix `main` too, as
`not found: value ClassLoaderUtil`).

So the correct rule runs first, and `expose_from_unopened_packages`, which looks at unopened
intermediate packages **only when nothing was found**, was kept. Everything that resolved before
still resolves; all that changes is **priority** -- and priority was precisely what the
`slick.cats` problem was about. Once default arguments either always become a call to their own
getter or are typed in the scope where they were written, this fallback can be deleted (the
comment says as much).

#### Verification

`pj_projmember.scala` / `pj_pkgscope.scala` pass `-Xverify:all` under both `--scala-library` and
`--no-scala-library`, and match real scalac 2.13.16's stdout. A qualified package clause needs
multiple files, so `a_qualified_package_clause_does_not_open_its_parent` writes out 5 of them to
check, and runs the same program through real scalac as well. `pj_projmember_bad.scala` rejects 3
cases, and nsc 2.13.16 reports the same 3. 4 of the 8 tests were confirmed to **fail on `main`
before the fix**. Because this touches the seam between package resolution and type projections,
`proj` / `cats2` / `tail4` / `tmember` / `conform` / `e2e` / `pkgalias` / `imports` / `multifile` /
`jarpickle` were run in `--release`. `cargo clippy --workspace --all-targets` reports 78 both
before and after, with zero new ones.

The one line in `pickle.rs` was confirmed to have an effect by compiling `pj_projmember.scala`
with and without it and observing that **the bytes of `Main.class` / `Main$.class` change**
(without it, the parameter type of `def db(ctx: Sub#Ctx)` comes out as a `REFINEDTPE` wrapped in
`<refinement>`). A black-box test that makes scalac read it back cannot be written -- there is a
separate hole where **nested classes are pickled with the empty package as their owner**, and
merely writing out `object Holder { class Inner(val n: Int) }` plus
`object Api { def take(i: Holder.Inner) }`, which has nothing to do with projections, and having
scalac read it via `-cp` gives `Symbol 'type <empty>.Inner' is missing from the classpath` /
`type Inner is not a member of object Holder` (see remaining issues below).

#### Known remaining issues

* Now that `cats.effect.IO` is visible, `cats.effect.IO(fa)` at `DBIOAction.scala:237` reports
  `no matching overload for IO$ with arguments (Future[R])` (2 cases became 1, for a net -1).
  This is a separate hole where `IO$`'s `apply[A](thunk: => A): IO[A]` cannot be supplied from
  the jar's pickle -- the same family as what `agent/cats2` fixed for `Async$`.
* The three cats `>>` cases (`no matching overload for (=> F[B])(FlatMap[F])F[B]`) are at three
  both before and after, and are **unrelated to type projections** (`agent/cats2`'s record was
  right). There is a separate reason the left-hand side falls to `Any` / `AnyRef`.
* (**One case resolved in `agent/tail6`**; the remaining 2 have different roots, as below.)
  The three `value map is not a member of Any` cases are untouched. They have nothing to do with
  projections or packages, and all three have different roots. Only one root was identified:
  `findFirstMatchIn(url).map(...)` at `DatabaseUrlDataSource.scala:31` is simply
  **`prelude_regex.rs` declaring `("findFirstMatchIn", vec![Type::String], Type::Any)`**. Six
  lines reproduce it (`val re = "a(b)c".r; re.findFirstMatchIn("abc").map(_ => "")`). Fixing it
  requires not only making it `Option[Regex.Match]` but also making the parameter
  `CharSequence`, matching the real ABI (see the comment on `unapplySeq` in the same file; as
  long as it stays `String` the descriptor does not match and it will not link). The remaining 2
  are `foundRefs.filter(...)` at `RewriteJoins.scala:139` and `prit` at
  `JdbcActionComponent.scala:162`, whose roots have not been traced.
* Default argument right-hand sides being typed at the call site (see 2 above) was diagnosed but
  not fixed. Fixing it should make both `expose_from_unopened_packages` and the pre-existing
  `not found: value ClassLoaderUtil` go away.
  -> **Resolved in `agent/tail6`** (both did go away, as predicted).
* A projection can only carry along **abstract type members**. A generic outer class (the `T` of
  `C[Int]#Inner` -> `Int`) cannot be carried, because of the way `RefineDecl` matches by name.
  This does not appear in slick.
* When the prefix is a class from a jar, its type members are not read until they are demanded,
  so things that could have been determined are sometimes missed (every affected place in slick
  is on the source side).
* **Nested classes are pickled with the empty package as their owner** (a separate hole found
  along the way, unrelated to this slice's changes). Two lines reproduce it: compile
  `object Holder { class Inner(val n: Int) }` / `object Api { def take(i: Holder.Inner): Int = i.n }`
  with scala-rs, put the classfiles on scalac's `-cp` and have it call `Api.take`, and it reports
  `type Inner is not a member of object Holder` and
  `Symbol 'type <empty>.Inner' is missing from the classpath`. Confirmed in `--scala-library`
  mode only. scala-rs reads its own output fine even via `-cp`, so the practical harm is limited
  to "when scalac reads it."

### Where default arguments get typed, `Regex`'s real ABI, and implicit parameters with defaults (`agent/tail6`)

The bug: three items `agent/proj` had left as "root identified but not fixed." The largest was that default argument right-hand sides were typed at the call site instead of in the scope where they were written, which meant names resolved against the caller's imports -- and could even resolve to a different symbol of the same name.

This slice handled the 3 items that `agent/proj` left behind as "root identified but not fixed."
`tests/slick_measure.sh` went **`errors=115 -> 110`, `files_with_errors=41 -> 39`** (with 0 new
errors). Codegen (`crates/backend/`) was not touched, so `tests/slick_subset.sh` was skipped.

#### 1. A default argument's right-hand side is typed in the **scope where it was written**

Defaults that cannot go through an `f$default$n` getter call -- in particular those on a primary
constructor (nsc emits getters on the companion; we do not synthesize them) -- had their
namer-saved tree spliced into the argument list and typed **at the call site**. As a result:

* Names resolved in the **caller's scope**. slick's
  `class DriverDataSource(..., classLoader: ClassLoader = ClassLoaderUtil.defaultClassLoader)` is
  written under `import slick.util.ClassLoaderUtil`, but
  `new DriverDataSource(..., driverObject = driver)` in `slick/jdbc/DatabaseConfig.scala` does not
  have that import, giving `not found: value ClassLoaderUtil`.
* Worse, the span stayed the **definition site's** while the file index was the caller's, so the
  caret landed on an unrelated line (`new DriverDataSource` at `DatabaseConfig.scala:48`). That
  is the proof that it is being typed at the call site.
* It can also turn into a different symbol with the same name.
  `actionListener: ActionListener[F] = defaultActionLogger[F]` at `BasicBackend.scala:69` was
  re-typed at `HeapBackend.scala:52`, so `F` became **HeapBackend's** `F`, giving
  `found: ActionListener[F]  required: ActionListener[F]`.

`Checker::record_default_scope` now remembers the scope stack, owner, `this_class` and unit at
the point of definition, and `type_default_rhs_here` restores them for typing. The typed tree
carries `NodeId::PRETYPED_DEFAULT`, and when `type_expr` sees that it **does not re-type**, only
`adapt`s (because on the named-argument path the caller's argument loop comes back to type it a
second time).

For constructor defaults, **the class's own member scope is excluded** from the remembered scope,
and the owner is moved outside the class. At the point of `new C(1)` there is no instance, so
neither fields nor preceding constructor arguments can be named -- and nsc is the same:
`class Pair(a: Int, b: Int = a)` is `not found: value a` in real scalac 2.13.16 as well (and the
same with `val a`). Leaving them in made `a` resolve to a **field**, so the spliced tree read it
off the caller's `this` and produced a `ClassCastException` at run time.

With this in place, the final fallback whose deletion condition `agent/proj` had left in a
comment, **`Checker::expose_from_unopened_packages`, was deleted**. As a side effect,
`enclosing_package_names_are_visible` in `crates/cli/tests/multifile.rs` fails. That fixture
(`tests/multi/pkg_inner.scala`) refers to `top.Helper` unqualified from `package top.inner`, and
**real scalac 2.13.16 rejects this** (`not found: value Helper`, with or without `-Xsource:3`).
Only the loose fallback was letting it through, so the fixture was changed to the nested spelling
`package top { package inner { ... } }` (the form nsc accepts; the qualified spelling is pinned
down by `crates/cli/tests/proj.rs`).

#### 2. Implicit parameters with defaults

When implicit search comes up empty, nsc **uses the default** if the parameter has one (it
reports `missing implicit` only when there is no default). slick's `ScalaBaseType` is written
assuming this --

```scala
def apply[T](implicit classTag: ClassTag[T], ordering: scala.math.Ordering[T] = null)
```

-- and `ScalaBaseType[T]` was reporting
`could not find implicit value of type Ordering[T]` (in two places in
`JdbcTypesComponent.scala`). `Checker::implicit_param_default` was added to
`fill_implicit_params_in`'s fallback chain (next to `ClassTag` / view / `TypeTag`). The default's
body is typed in the **scope where it was written**, same as in 1.

#### 3. `prelude_regex` was masking the jar's signatures

Besides `unapplySeq`, `prelude_regex.rs` declared `findAllIn` / `findFirstMatchIn` /
`replaceAllIn` / `replaceFirstIn` / `split` as "fallbacks for when there is no pickle." But
**a jar's members are invisible to `lookup_member` until someone demands them**, so the
`is_empty()` guard at install time was always true -- meaning **the fallback was always the real
thing**. Two consequences:

* The result types of `findAllIn` / `findFirstMatchIn` were `Any`. So
  `MysqlCustomProperties.findFirstMatchIn(url).map(...)` gave
  `value map is not a member of Any` (`DatabaseUrlDataSource.scala:31`).
* Even the ones that did have a usable result type had `String` parameters. The real ABI is
  `CharSequence`, so the descriptor does not match: compilation succeeds and then fails at run
  time with `NoSuchMethodError: Regex.replaceAllIn(String, String)`.

**All five were deleted.** The pickle can supply all five with their real signatures (only
`unapplySeq` is not supplied, so it is kept). Names that cannot be supplied are now diagnosed as
"not a member of `Regex`" -- more honest than silently handing out a lying type.

The remaining 2 `value map is not a member of Any` cases (`foundRefs.filter(...)` at
`RewriteJoins.scala:139` and `prit` at `JdbcActionComponent.scala:162`) are **not the same
species**. As `agent/proj`'s record said, the three had three different roots.

#### 4. A jar-derived implicit candidate only matches its own type until its parents are read

Under `class C[F[_]](implicit F: Async[F])`, `implicitly[Sync[F]]` gave
`could not find implicit value of type Sync[F]`. `Async`'s parent list stays empty for a class
whose **name the program merely mentions**, and implicit search runs under an immutable borrow,
so it cannot complete it itself. Simply writing `Async[F]` as a type on an earlier line in the
same file makes it work -- so the shape is a completion gap, not a scoping rule.
`Checker::warm_implicit_candidates` was added; it runs **only after** a search comes up empty.
Standard library classes are excluded: re-adding parents from the classfile rewrote
`mutable.HashSet`'s hierarchy and added 2 `containsSymbol(Set[A])` overload errors in slick
(the same trap the comment on `warm_own_scope_once` warns about).

#### Fixture and tests

* `tests/fixtures/t6_defaults.scala` (+ `expected/`) -- that a default's right-hand side
  resolves in the definition scope (four paths: positional and named `new`, an ordinary method,
  and an implicit parameter with a default). Both modes.
* `tests/fixtures/t6_defaults_bad.scala` -- a name not in the definition scope is an error
  (`Hidden`), and a constructor default cannot see preceding constructor arguments either (`a`).
  A separate test pins down that **real scalac reports the same 2**.
* `tests/fixtures/t6_regex.scala` (+ `expected/`) -- the 7 `Regex` methods plus `unapplySeq`.
  Jar mode only (the private runtime has no `Regex`, so the diagnostic is pinned down instead).
* `an_implicit_from_a_jar_answers_for_its_supertypes`, which runs only when the cats-effect jar
  is in the Coursier cache.

The tests are the 9 in the new file `crates/cli/tests/tail6.rs`.
**5 of them were confirmed to fail on pre-fix main.**

#### Remaining

* **The 2 `GenTemporal[F, _]` cases were left as they are** (`wait.timeoutTo(timeout, ...)` in
  `ConcurrencyControl.scala`). The fix in 4 makes `implicitly[GenTemporal[F, Throwable]]` work,
  but the `E` in `timeoutTo[B >: A, E](...)(implicit F: GenTemporal[F, E])` is a type parameter
  that appears only in the implicit clause, and it is **collapsed to `Type::Wildcard`** before
  the search is ever reached (`GenTemporal[F, _]`). **Writing the type arguments explicitly as
  `timeoutTo[Unit, Throwable]` makes no difference**, so the collapse happens earlier than
  `solve_implicit_only_tparams` / `adapt_implicit_apply` -- at the stage where the `Select` on the
  `GenTemporalOps_` obtained from `cats.effect.syntax`'s implicit conversion is typed.
  `Wildcard` erases variable identity, so no candidate can be matched.
* `Ordering[Null]` (`Type.scala:395`, `new ScalaBaseType[Null]`) is a call that genuinely
  requires an `Ordering[Null]` rather than using a default; it is a different root.
* nsc's companion getters for constructor defaults (`C$default$n`) are still not synthesized. As
  noted above, preceding constructor arguments cannot be referenced in nsc either, so there
  should be no observable difference, but defaults cannot be filled in across a jar under
  separate compilation.

### The shape of a function literal, before its parameter types (`agent/gbovl`)

Overloading on nothing but the *arity* of a function-literal argument:

```scala
def only(action: Repo => Any): String
def only[T](action: (T, Repo) => Any): T => String
only { r => r.nm }              // the first
only[T] { (form, r) => … }      // the second
```

The literal cannot be typed until the alternative is picked, and the
alternative cannot be picked from a literal that is not typed. nsc breaks the
circle with `Infer.shapeType`, which it feeds to `isApplicableSafe` before
`typedArgs`: a `Function(vparams, body)` becomes
`functionType(vparams map (_ => AnyTpe), shapeType(body))`, so the parameter
types are `Any` and the **arity is the source's**. A `{ case … }` literal
(`Match(EmptyTree, _)`) becomes `PartialFunction[Any, Nothing]` — arity one,
which is why `g { case (n, s) => s }` against a `Function1`/`Function2` pair
picks the `Function1` in real scalac and then fails.

`arg_score` here deliberately lets an un-inferred literal match a function
parameter of *any* arity, because a one-parameter `{ case … }` literal really
does inhabit an `(A, B) => C` by tupling when that is the only candidate. So
the arity is applied one level up, in `Typer::narrow_by_lambda_shape`, as a
filter on the *set* of applicable alternatives: it runs only when two or more
are applicable, and it keeps the whole set when it would empty it. It can
therefore turn an ambiguity into a pick, never a pick into a failure. The full
account, with the two other roots the same gitbucket symptom turned out to
hide (a `-cp` class that is only ever inferred is never completed; and
dropping an argument's prototype has to *earn* it), is in `docs/gitbucket.md`
under root 17.
