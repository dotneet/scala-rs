## Testing

```bash
cargo test
```

The regression tests for the pickle reader are in
`crates/pickle/tests/lib_jar.rs`. When
`/tmp/scala-rs-lib/scala-library-2.13.16.jar` (or `SCALA_LIBRARY_JAR`) is
present, they scan **every class file** in the jar and check the following. If
the jar is absent they skip.

- `reads_every_pickle_in_scala_library`: **every** pickle in the class files
  that declare `@ScalaSignature` / `@ScalaLongSignature` (799 of 2891 in
  2.13.16) can be read. Whether a class file "declares" one is decided by an
  independent check — a byte search for the descriptor in the constant pool — so
  a failure to extract one is a failure too. 169275 entries in total. It also
  checks that the major tags actually occur. On top of that it builds class
  signatures from all the pickles (2209 classes) and checks there are **zero
  unresolved references** (`ClassSig::unresolved` is empty).
- `list_pickle_has_the_collection_members`: `List` and `map` out of
  `List.class`'s pickle.
- `resolves_inherited_list_members_through_parents`: `List#filter` / `sum` /
  `mkString` / `map` / `flatMap` / `head` / `foldLeft` resolve **by walking the
  parent classes**.
- `resolves_module_class_members`: resolution for a module class (`object List`).
- `set_filter_binds_c_through_setops_not_iterable` /
  `linearization_puts_later_parents_first`: the search order is SLS 5.1.2
  linearization (`Set#filter` returns `Set[A]`, not `Iterable[A]`).
- `flag_bits_match_the_library`: pins the position of every flag bit in a pickle
  against real symbols (trait / accessor / stable / synthetic / private+local /
  default argument).

The test that reads back a pickle our own writer produced with our own reader is
`crates/backend/tests/pickle_roundtrip.rs`.

The tests for **reading classes out of a jar through their pickles** are in
`crates/cli/tests/jarpickle.rs` (fixture prefix `jarpk`).

- `jarpk_fixture_dual_run`: compiles `jarpk.scala` (`Functor[F[_]]` /
  `Monadic[F[_]]` with three instances — `Option`, `List` and a hand-written
  `Ident`) against the real scala-library and checks it matches real scalac
  2.13.16's output (`tests/fixtures/expected/jarpk.txt`) **exactly**.
- `jarpk_bad_is_still_rejected`: the kind error in `Monadic2[Int]` and the type
  mismatch in `F.pure(1): F[String]`. nsc 2.13.16 rejects both too.
- `a_higher_kinded_trait_survives_a_jar_round_trip`: compiles a library
  containing a higher-kinded trait, packs it with `jar cf`, then compiles and
  runs a program that can see **nothing but the jar**. All that crosses is the
  `ScalaSignature`. Skipped when `jar` is unavailable.
- `a_higher_kinded_type_class_from_a_real_jar_typechecks` /
  `a_proper_type_is_still_rejected_where_a_real_jar_wants_a_constructor`: when
  cats-core / cats-kernel are in the local Coursier cache, checks that
  `F.pure` / `F.flatMap` / `F.map` typecheck against the real `cats.Monad`, and
  that `Monad[Int]` is a kind error. Skipped otherwise (nothing is downloaded).

The hook into type checking is covered by `crates/cli/tests/pickle_lib.rs`
(fixture prefix `pickle_lib`), kept in a separate file from `e2e.rs`.

- `pickle_lib1` (inherited members) / `pickle_lib2` (`Ordering`, type aliases,
  currying) / `pickle_lib3` (linearization and stubbed classes) / `pickle_lib4`
  (operators, companions, `sum`): compile linked against the jar and compare the
  expected stdout under `java -Xverify:all`. **All four expected values have been
  confirmed byte-identical to real scalac 2.13.16's output** (this is not a
  comparison of our compiler against itself).
- `a_member_in_no_pickle_is_still_an_error` (`pickle_lib1_bad`): a name that is
  in no pickle is not filled in, and becomes `is not a member`.
- `private_runtime_still_diagnoses_library_only_members`: under
  `--no-scala-library` there are no pickles to read, so it diagnoses properly
  rather than quietly accepting.

The invariants of the fill-in are pinned by unit tests in
`crates/typer/src/pickle_supply.rs`: `the_prelude_wins_over_the_pickle` (the
hand-written `List#map` is neither replaced nor duplicated, while `filter`,
which the prelude lacks, is supplied with a descriptor) and
`nothing_is_supplied_when_nothing_is_missing` (no prefetching).

Runtime expectations live in `tests/fixtures/`. For each `.scala` there is a
`.txt` of the same name in `tests/fixtures/expected/` (stdout, with the trailing
newline `println` produces). Where `java` is available, the CLI's e2e tests
compare stdout.

Where scala-library 2.13.16 can be obtained, the fixtures are also compiled with
`--scala-library` and run as `java -cp out:scala-library.jar Main`, checking that
the stdout matches the private-runtime version (and that no private
`scala/Option.class` / `scala/Predef$.class` and the like are emitted). The
authoritative list of fixtures covered is the `scala_library_dual_run_*` tests in
`crates/cli/tests/e2e.rs`. A `compile` with no flags auto-detects the jar and
links against it; `--no-scala-library` emits the private runtime.

The regression tests for passing several files to a single `compile` are in
`crates/cli/tests/multifile.rs`, with sources in `tests/multi/`. The cake-pattern
fixtures use the prefix `cake` (`cake_profile.scala` / `cake_relational.scala` /
`cake_component.scala` for the good cases, `cake_bad_leaf.scala` /
`cake_bad_base.scala` for the bad ones); the good cases are also checked to give
the same result **when the file order is permuted**. The bad cases pin that names
absent from the linearization (`Missing`, which exists nowhere, and `Detached` on
a component that was not mixed in) do not quietly pass. Both have been confirmed
to match real scalac 2.13.16 in output and diagnostics.

The fixtures that closed holes in the prelude and small holes in type checking
use the prefix `gap_` (`gap_numeric` / `gap_asinstanceof` / `gap_copy` /
`gap_exception`, each with a `_bad` counterpart) and live in
`crates/cli/tests/gaps.rs` rather than `crates/cli/tests/e2e.rs` (so that other
agents editing `e2e.rs` at the same time do not conflict). Besides the
`--scala-library` dual run, where `scalac` is available they diff the actual
output directly against real scalac's on every run (the `expected/*.txt` were
produced from real scalac's output). `gap_copy` also works on the private
runtime.

The fixtures for boxed types (keeping `java.lang.Integer` and `scala.Int`
separate) use the prefix `boxed` and live in `crates/cli/tests/boxed.rs` for the
same reason. `boxed.scala` gets both the `--scala-library` dual run and the
output diff against real scalac; `boxed_rt.scala` covers the part that also works
on the private runtime (the conversion intrinsics and the JDK wrapper classes)
and is run both ways. `boxed_bad.scala` diagnoses, in both jar mode and private
runtime mode, the five conversions real scalac rejects (`java.lang.Integer = 3L`
/ boxed `Long` → `Integer` / boxed `Long` → `Int` / boxed → `String` / a static
`parseInt` through an instance). That `scala.Int` and `java.lang.Integer` are
separate symbols is also checked by the typer-side invariant test
`prelude_has_no_duplicate_jvm_classes` (restated as: the only symbols allowed to
share a JVM name are a value class and its box, and the box side belongs to
`java.lang`).

The fixtures for the numeric conversion tower and the primitivisation of `Byte` /
`Short` use the prefix `numt` (`numt.scala` / `numt_bad.scala`) and live in
`crates/cli/tests/numtower.rs` for the same reason. `numt.scala` gathers all 7×7
conversions (including NaN / ±Inf / MIN and MAX), `Byte` / `Short` as parameters,
return values, fields, array elements and overflow, operator promotion, weak
conformance, and `Int` constant patterns in a `Short` scrutinee, and runs the lot
under `java -Xverify:all` in **both the private runtime and `--scala-library`**,
comparing against `expected/numt.txt` (real scalac 2.13.16's stdout).
`no_scala_byte_or_short_class_reference` directly checks that the class names
`scala/Byte` / `scala/Short`, which do not exist, never appear in the constant
pool of the emitted class files. `numt_bad.scala` diagnoses, in both jar mode and
private runtime mode, what real scalac also rejects (implicit narrowing,
out-of-range constants, `toX` on `Boolean` / `Unit`, `Double` → `Int`). The
element instructions for primitive arrays (`laload` / `dastore` / `baload`, …)
and the `i2f` in `1 + 2.5f` are pinned by separate tests.

The fixtures for the `agent/product` slice (`case class` / `case object`
implementing `scala.Product`, and the synthetic companion extending
`scala.runtime.AbstractFunctionN`) use the prefix `prod` (`prod` / `prod_lib` /
`prod_vc` / `prod_bad`) and live in `crates/cli/tests/product.rs` for the same
reason. `prod.scala` runs the four overridden accessors (`productPrefix` /
`productArity` / `productElement` / `productElementName`) and three kinds of
out-of-range access (above, negative, arity 0) under `java -Xverify:all` in
**both the private runtime and `--scala-library`**, and
`real_scalac_dual_run_prod` compares against real scalac 2.13.16's stdout
(`expected/prod.txt` is scalac's output verbatim — down to `case class Zero()`
and `case object Solo` producing **different out-of-range messages**).
`prod_vc.scala` pins, in the same three modes, that a value class's field gets
re-wrapped into an instance by `productElement`. `prod_lib.scala` deals with
`Product` as a **type**, `productIterator` / `productElementNames`, `tupled` /
`curried`, `val f: (Int, String) => P = P`, and arity 22, so it only gets the
library dual run and the real scalac dual run;
`fixtures_prod_lib_without_library_is_error` checks that under
`--no-scala-library` those are **properly diagnosed**.
`prod_lib_classfile_shape` pins the exact shape `javap -p -c` prints
(`implements scala.Product,java.io.Serializable`, `tableswitch`,
`Statics.ioobe`, `ScalaRunTime$.typedProductIterator`,
`Product.productElementNames$`, the companion's `extends AbstractFunction2` and
erased `apply` bridge, `AbstractFunction22`, a case object **not** extending
`AbstractFunctionN`, and a case object's `productElementName` being a forwarder
to `Product.productElementName$`). `prod_bad.scala` diagnoses the four things
real scalac also rejects (`productArity` / `productElement` on a non-case class,
`productElement("0")`, `val bad: Product = new Plain(1)`).

The fixtures for the `agent/smallgaps` slice (placement of `@inline` /
`@noinline`, curried case class companions, backward references to a companion,
the polymorphism of `Option.flatMap`, the `lub` of `None`/`Some`,
`Iterable.apply`) use the prefix `sgap` (`sgap` / `sgap_lib`) and live in
`crates/cli/tests/smallgaps.rs` for the same reason. `sgap.scala` is `check`ed
under `--no-scala-library`; `sgap_lib.scala` is library-dual-run only, because
`Iterable.apply` exists only in the library ABI (inherited from
`IterableFactory$Delegate.apply`), and
`fixtures_sgap_lib_without_library_is_error` also checks that
`--no-scala-library` keeps diagnosing it.

The fixtures for the `agent/anonbridge` slice (values of `Block` / `If` /
`Match` / `Try` being boxed twice after erasure) use the prefix `ab` (`ab` /
`ab_bad`) and live in `crates/cli/tests/anonbridge.rs` for the same reason.
`ab.scala` gathers block bodies for all eight primitives, implementations in an
`abstract class` and in a named class, primitive parameters, two type parameters,
a generic applied to a generic, an implementation by `val`, a SAM-converted
lambda, `while` / `if` / `match` / `try` bodies, a captured `var`, `val x: Any =
{ … }` / `id({ … })` without an anonymous class, and the opposite direction
(`val n: Int = { val z: Any = 1; z.asInstanceOf[Int] }`), and runs it under `java
-Xverify:all` in **both the private runtime and `--scala-library`**, comparing
against `expected/ab.txt` (real scalac 2.13.16's stdout, via
`real_scalac_dual_run_ab`). `erased_next_boxes_its_block_exactly_once` and
`scalac_and_ours_agree_on_the_erased_entry_point` use `javap -p -c -s` to check
directly that there is **exactly one boxing inside `next()Ljava/lang/Object;`**,
and that real scalac has the same entry point (in its own different shape:
`next()I` plus a bridge). Double boxing is invisible in the runtime output alone,
so the `javap` side is pinned separately. `ab.scala` has no `Unit` in it because
`()` never occurs in a reference position, so no boxing happens and it would
instead hit a **separate, unfixed** issue (see Remaining, below). `ab_bad.scala`
pins that boxing does not swallow a type mismatch (the same
`type mismatch; found: String  required: Int` as real scalac).

The fixtures for the `agent/stringops8` slice (filling in `StringOps` from the
jar's `ScalaSignature`) use the prefix `so8` (`so8` / `so8_bad`) and live in
`crates/cli/tests/stringops8.rs` for the same reason. `so8.scala` is library dual
run only, since `StringOps` exists only in the library ABI, and **its expected
value is real scalac 2.13.16's stdout verbatim** (matched under `java
-Xverify:all`). `fixtures_so8_without_library_is_error` checks that
`--no-scala-library` keeps producing all 40 diagnostics (does not quietly
accept), and `fixtures_so8_bad_collect_result_type_is_error` pins that resolving
a result-type-only overload is not enough on its own: a `collect` whose case
block returns `Int` cannot be bound to `String`.

The fixtures for the `agent/durrange` slice (postfix units of
`scala.concurrent.duration`, the `Range` companion's `apply` / `inclusive`, and
the view path that fills a function-typed implicit parameter from an implicit
def) use the prefix `dr` (`dr_duration` / `dr_range` / `dr_view` /
`dr_viewuser` / `dr_view_bad`) and live in `crates/cli/tests/durrange.rs` for the
same reason. `dr_duration.scala` covers all 20 unit methods of `DurationInt` /
`DurationLong` / `DurationDouble` plus `FiniteDuration` arithmetic;
`dr_range.scala` covers every overload of `Range$`'s `apply` / `inclusive` /
`count` (only the `Int` versions in `javap`); `dr_view.scala` covers eta-expanding
`Ordered.orderingToOrdered` and passing it, and view bounds. These three are
backed only by the real library jar, so they are library dual run only, and the
`expected/*.txt` are real scalac 2.13.16's stdout verbatim.
`fixtures_dr_*_without_library_is_error` checks they are **properly diagnosed**
under `--no-scala-library`. `dr_viewuser.scala` writes the same view path using
nothing but user-written `implicit def`s (monomorphic, polymorphic, with their
own implicit clause, view bounds, nested implicit parameters), and runs in **both
the private runtime and `--scala-library`** (confirming that path is not jar
dependent). `dr_view_bad.scala` pins that types with no witness (`Plain` /
`Object`) are rejected in both modes (corresponding to real scalac's
`No implicit view available from Plain => Ordered[Plain]`).
`dr_noimpl_bad.scala` pins, in both modes, that **a method taking only implicits
is a type error when they cannot be filled** (it is not quietly eta-expanded into
a function value).

The fixtures for the `agent/catsimpl` slice (a lambda capturing the enclosing
`this`, cats-style syntax implicit conversions, companion implicit scope, by-name
arguments in a call that omits a default argument) use the prefix `cats`
(`cats_lambda` / `cats_lambda2` / `cats_syntax` / `cats_syntax_bad` /
`cats_byname`) and live in `crates/cli/tests/catsimpl.rs` for the same reason.
`cats_lambda.scala` uses `List.map` / `flatMap`, so it is library dual run only;
`cats_lambda2.scala` writes the same capture without library collections and so
runs in **both the private runtime and `--scala-library`**. `cats_syntax.scala` is
a single-file version with a hand-written
`implicit def toFlatMapOps[F[_], A](fa: F[A])(implicit F: FlatMap[F])`, exercising
both an abstract `F[_]` and a concrete `Box` as the receiver.
`cats_syntax_bad.scala` pins that widening the conversion's parameter to "any
type applied to one argument" did **not** make the conversion apply to types with
no witness (the same `value flatMap is not a member of Bag[Int]` as scalac).
`a_higher_kinded_companion_implicit_crosses_a_jar` compiles a library itself,
packs it into a jar, and checks both that `Async[Box]` = `Box.asyncForBox` is
found through the `ScalaSignature` alone, and that a type with **no witness is
still a hard error** (`could not find implicit value of type Async[Crate]`).

The fixtures for the `agent/catsyntax` slice (extension methods via cats syntax
reaching real cats) use the prefix `csyn` (`csyn_ops` / `csyn_ops_bad`) and live
in `crates/cli/tests/catsyntax.rs` for the same reason. `csyn_ops.scala` calls
`map` / `flatMap` / `foreach` on a receiver shaped like cats' `Ops[F[_], A]`,
**using no implicit conversion at all** (`new Ops[Box, Int](b)`), pinning the
discrepancy where the lambda's parameter type became the first type argument
`Box`. It runs in both the private runtime and `--scala-library`.
`csyn_ops_bad.scala` pins that giving the lambda its declared parameter type does
not make a call without a witness compile
(`could not find implicit value of type FlatMap[Bag]`).
`a_simulacrum_style_syntax_layer_crosses_a_jar` uses **real scalac** to compile a
miniature cats (a refinement result type
`Ops[F, A] { type TypeClassType = FlatMap[F] }`, a nested `object all` in a
package object, and an unrelated class that merely puts that `all` into
`InnerClasses`), packs it into a jar, and checks that `b.flatMap(…)` and `b >> …`
resolve through the `ScalaSignature` alone and run under `java -Xverify:all`. Our
own pickle writer does not emit `REFINEDtpe`, so this fixture is only meaningful
if scalac wrote it (skipped where scalac is unavailable). The same test also
checks that the conversion does not apply to `Crate`, which has no witness
(`value flatMap is not a member of Crate[Int]`).

The fixtures for the `agent/cats2` slice (cats-effect's summoner returning
`F.type`, and `$this` in string interpolation) use the prefix `c2`
(`c2_thisinterp` / `c2_thisinterp_bad`) and live in `crates/cli/tests/cats2.rs`
for the same reason. `c2_thisinterp.scala` puts `s"… $this …"` inside a class, a
trait, an `object` and a lambda, and runs under `-Xverify:all` in both the
private runtime and `--scala-library`, matching real scalac 2.13.16's output.
`c2_thisinterp_bad.scala` pins that special-casing `$this` did not make `$name`
accept anything at all (`not found: value nosuchvalue`).
`a_summoner_returning_its_own_parameters_type_crosses_a_jar` uses **real scalac**
to compile a small library with a cats-effect-shaped summoner
(`def apply[F[_]](implicit F: TC[F]): F.type = F`) and a package object holding
`val TC = tinyeff.TC` (exactly the path that makes `import cats.effect.Async`
work), packs it into a jar, and checks that `TC[G].flatMap(fa)(…)` resolves
through the `ScalaSignature` alone and runs under `java -Xverify:all`. Our own
pickle writer does not write a `SINGLEtype` pointing at a parameter, so this
fixture is only meaningful if scalac wrote it (skipped where scalac is
unavailable). The same test also checks that `TC[Crate]`, which has no witness,
still gives `could not find implicit value of type TC[Crate]`.

The fixtures for the `agent/cats3` slice (a by-name formal parameter not becoming
a prototype, and subsequent clauses of an overloaded member being re-read from
the declaration) use the prefix `c3` (`c3_infer` / `c3_infer_bad`) and live in
`crates/cli/tests/cats3.rs` for the same reason. `c3_infer.scala` puts two roots
side by side without a single line of cats: passing
`good.fold(boom, _ => new Box(()))` to
`def >>[B](fb: => F[B])(implicit ev: Bind[F]): F[B]` (the expected type says
`B = Unit`, so the by-name formal becomes the argument's prototype as is), and
the shape where an **overloaded** `tag`'s `implicit t: TC[F, _]` — as with
`Duration` / `FiniteDuration` — is read at the receiver's `F`. It runs under
`-Xverify:all` in both the private runtime and `--scala-library`, and
`scalac_agrees_c3_infer_output` checks it also matches real scalac 2.13.16's
stdout (**on main before the fix it fails with 4 errors**, two of them
`could not find implicit value of type TC[F, _]` — the same `GenTemporal[F, _]`
slick was reporting, the declaration's `F` rather than the receiver's).
`c3_infer_bad.scala` pins that the prototype is not a licence to accept anything
— a `val` inferred earlier **without** an expected type is still a
`type mismatch` — and that a witness for a different type constructor is still
not found (`could not find implicit value of type TC[Box, _]`), with
`scalac_agrees_c3_infer_bad_is_rejected` checking that real scalac also reports
the same 2 errors on the same 2 lines. `cats_flat_map_then_and_timeout_to_compile`
runs only when cats-core / cats-kernel / cats-effect{,-kernel,-std} are in the
Coursier cache, and checks that `a >> e.fold(F.raiseError, _ => F.unit)` and
`wait0.timeoutTo(timeout, F.raiseError[Unit](…))` — the exact shapes from slick's
`BasicBackend.scala` and `ConcurrencyControl.scala` — compile **against real
cats** (`scalac_agrees_cats_flat_map_then_and_timeout_to` puts the same 11 lines
through real scalac). `cats_syntax_conversion_completes_its_own_witness` compiles
the third root — `trait C3Db[F[_]] { implicit val asyncF: Async[F]; def run(fa:
F[Long]) = fa.flatMap(…) }` (slick's `BasicDatabaseDef`) — **as a compilation
unit on its own**. Adding a single line that touches `Async` makes it compile
even before the fix, so being alone is the reproduction condition.

The fixtures for the `agent/companionkind` slice (a companion and its class
sharing one symbol) use the prefix `ckind` (`ckind_future` /
`ckind_future_bad`) and live in `crates/cli/tests/companionkind.rs` for the same
reason. `ckind_future.scala` calls `Future.apply`, the **by-name member of the
companion** of `scala.concurrent.Future` — a class the prelude does not have and
whose members all come from the jar. The JVM's generic signature cannot express
by-name, so it became `Function0[T]` and `Future(21)` failed with
`no matching overload for (Function0[T], ExecutionContext)Future[T]`. It is
checked both by the `--scala-library` dual run and by a direct output diff
against **real scalac 2.13.16** (`real_scalac_dual_run_ckind_future`); it is not
run under `--no-scala-library`, since `scala.concurrent` is not in the private
runtime. `ckind_future_bad.scala` pins that now the signature is the real one,
**its implicit clause is real too**: without an `ExecutionContext` in scope it is
rejected, as scalac does. `a_companion_and_its_class_are_separate_symbols` uses
**real scalac** to build a shrunken-cats jar (a higher-kinded trait
`Ref[F[_], A]`, its companion, and a package object with `val Ref = tinyeff.Ref`
and `type Ref[F[_], A] = tinyeff.Ref[F, A]`) and checks that the result type of
`r.update(_ + 1)` is `F[Unit]` (not the bare `F` from the class file), that the
companion's `Ref.const` can be reached without being mixed into the trait side,
and that the nonexistent name `bogus` is properly rejected.

The fixtures for the `agent/ambigmap` slice (two copies of the same pickled
declaration being installed, giving `ambiguous overload for map`) use the prefix
`am` (`am_pickledup` / `am_pickledup_bad`) and live in
`crates/cli/tests/ambigmap.rs` for the same reason. In `am_pickledup.scala`
**the order of the three blocks is itself the reproduction condition**: first a
`scala.Seq` receiver asks for `map`, then a `scala.collection.IndexedSeq`
receiver asks, and last `scala.IndexedSeq`, which has both as parents. It puts
not only `map` but also `flatMap` / `filter` / `partition` / `foldLeft` through
the same three receivers, so it is visible that the fix is not a special case for
`map`. It runs under `java -Xverify:all` both in the `--scala-library` dual run
and in an output diff against **real scalac 2.13.16**
(`real_scalac_dual_run_am_pickledup`) — the reinstalled symbols change the callee's
owner and descriptor, so getting past the verifier is itself the check. The
private runtime has no `scala.collection` and no pickles (hence no copies to
merge), so `am_pickledup_without_the_library_is_diagnosed` pins that
`--no-scala-library` **produces a diagnostic instead of quietly accepting**.
`am_pickledup_bad.scala` pins that what is being merged is declarations, not
names: two genuine overloads stay two, and if nothing settles it, it is rejected
as scalac does.

The fixtures for the `agent/buildfrom` slice (a conversion method's **result
type** not being narrowed to the receiver's collection) use the prefix `bf`
(`bf_curried` / `bf_coll` / `bf_coll_bad`) and live in
`crates/cli/tests/buildfrom.rs` for the same reason. `bf_curried.scala` runs under
`java -Xverify:all` in **both the private runtime and `--scala-library`** (it is
written with unary functions only, since `scala.Function2` is not in the private
runtime). `bf_coll.scala` has result types that are all real `scala.collection`
classes, so it is jar only, and its output matches **real scalac 2.13.16's output
verbatim** (`expected/bf_coll.txt`). `bf_coll_without_library_is_error` pins that
the private runtime's lack of `MapOps` / `Factory` / `TreeMap` is **diagnosed
rather than quietly accepted**. `bf_coll_bad.scala` pins the **three things
narrowing must not accept** — `Map.map` with a lambda that does not return pairs
is an `Iterable`, `to(ArrayBuffer)` is not a `List`, and `groupMapReduce`'s value
type is what the second clause returns — rejected for the same reasons scalac
gives. There are 9 more unit-ish cases; in particular
`bf_plus_minus_on_non_collections_is_untouched` (every receiver goes through this
path for `+` / `-`, so arithmetic and string concatenation must be untouched) and
`bf_user_subclass_does_not_rebuild` (nothing is rebuilt unless it is a
`scala.collection` class) are what guarantee the fix is not a per-symptom special
case.

The fixtures for the `agent/hkinfer` slice (inferring type arguments from an
argument's base type, and auto-tupling at an overloaded callee) use the prefix
`hk` (`hk_base` / `hk_base_lib` / `hk_base_bad` / `hk_tuple` / `hk_tuple_lib` /
`hk_tuple_bad`) and live in `crates/cli/tests/hkinfer.rs` for the same reason.
`hk_base.scala` and `hk_tuple.scala` run under `java -Xverify:all` in **both the
private runtime and `--scala-library`**. `hk_base_lib.scala` (`Option` / `List`)
and `hk_tuple_lib.scala` (`println(1, "a")`) are jar only. There are two bad
cases, and both pin the error count **in both modes**: `hk_base_bad` for base
types whose type arguments do not match, and `hk_tuple_bad` for the four shapes
tupling must not accept (notably the reverse direction `g((1, 2))`, and `c(1,
"x")` when a candidate of the same arity exists). See "argument base types and
auto-tupling" below for the details.

The fixtures for the `agent/genrep` slice (the holes that had to be closed for
the 7 files slick generates from `.fm` templates: class type parameter bounds
that ignore imports, `implicit class` with type parameters, `TupleN extends
Product`, the type of an inherited overload at the receiver, tupling of argument
lists, class names that merely start with `Tuple`, varargs constructors, wildcard
type arguments and contravariance, top-level definitions after `package p { … }`)
use the prefix `genrep` (`genrep` / `genrep_bound_bad` / `genrep_tuple_bad` /
`genrep_product_bad`) and live in `crates/cli/tests/genrep.rs` for the same
reason. Besides the `--scala-library` dual run, `genrep.scala` is also checked by
an output diff against real scalac 2.13.16 (`real_scalac_dual_run_genrep`). There
are three bad cases: `genrep_bound_bad` pins that **a nonexistent type is still
properly diagnosed** even under the bound the namer was made to keep quiet about,
`genrep_tuple_bad` that tupling **does not accept wrong calls**, and
`genrep_product_bad` that no `Product` edge is added under `--no-scala-library`
(nothing in the private runtime backs it).

The fixtures for the `agent/ctoraccessor` slice (accessors for constructor
parameters, `FunctionN.tupled` / `curried` / `Function.untupled`, `Builder`'s
`+=` / `++=`) use the prefix `ctacc` (`ctacc` / `ctacc_fn` / `ctacc_builder` /
`ctacc_plain_bad`) and live in `crates/cli/tests/ctoraccessor.rs` for the same
reason. `ctacc.scala` runs under `java -Xverify:all` in **both the private
runtime and `--scala-library`**, and `real_scalac_dual_run_ctacc` also compares
against real scalac 2.13.16's output (`expected/ctacc.txt` is scalac's output
verbatim). `ctacc_case_class_params_get_public_accessors` uses `javap -p -s` to
pin the accessors' descriptors (`ConstRep.value()Object` / `NumRep.n()I` /
`IntBox.unwrap` as `()I` plus an `()Object` bridge / `StringBox.label` as
`()String` plus an `()Object` bridge) and that **the second parameter list does
not become accessors** (`Multi.extra`). `ctacc_fn.scala` and
`ctacc_builder.scala` are limited to the library ABI (`scala/FunctionN`'s default
methods, `scala/Function$`, `Growable`), so they only get the library dual run
and the real scalac dual run, with
`fixtures_ctacc_fn_without_library_is_error` /
`fixtures_ctacc_builder_without_library_is_error` checking they are **properly
diagnosed** under `--no-scala-library`. `ctacc_plain_bad.scala` pins that a
constructor parameter without `val` remains unreadable from outside (only a case
class's first parameter list becomes accessors).

The fixtures for the regression where an overload set disappears when another
class is loaded use the prefix `oshadow` (`oshadow` / `oshadow_java_first` /
`oshadow_java_last` / `oshadow_bad`) and live in
`crates/cli/tests/overloadshadow.rs` for the same reason. Besides the
`--scala-library` dual run, `oshadow.scala` is compared directly against real
scalac 2.13.16's output (`oshadow_matches_scalac`). `oshadow_java_first.scala` and
`oshadow_java_last.scala` are the same program with only the position of
`java.math.BigDecimal` swapped, and `oshadow_order_independent` pins that both
compile and that their stdout agrees. `oshadow_bad.scala` checks that
`BigDecimal(Some(1))` (which real scalac also rejects) becomes
`no matching overload` and that **the whole candidate list** is printed
(including `(String)BigDecimal`). `oshadow_without_library_is_error` checks that
the `not found: value BigDecimal` diagnostic remains under `--no-scala-library`.

The fixtures for the `agent/parentimpl` slice (filling in a parent constructor's
implicit clause and default arguments) use the prefix `pimpl` (`pimpl` /
`pimpl_bad`) and live in `crates/cli/tests/parentimpl.rs` for the same reason.
`pimpl.scala` gathers slick's `ConstColumn` shape
(`class ConstColumn[T : TT] extends TypedRep[T]`), an explicit clause plus a
two-argument implicit clause, all-default and trailing-default parameters, a
default clause plus an implicit clause, an anonymous class as the parent, and a
`new` with no arguments, and runs under `java -Xverify:all` in **both the private
runtime and `--scala-library`**. `real_scalac_dual_run_pimpl` runs the same source
through real scalac 2.13.16 and checks the stdout agrees (`expected/pimpl.txt` is
scalac's output verbatim). `pimpl_late_a.scala` / `pimpl_late_z.scala` compile
**the child before the parent** to check that the evidence for the parent's
context bound is filled in even when it does not exist yet at the signature pass
(that is, no dependence on file order). `pimpl_bad.scala` pins that a parent
implicit clause with no witness **does not quietly pass**, and
`pimpl_bad_reports_the_extends_clause_once` also checks the diagnostic appears
exactly once, on the `extends` line (not multiplied by three passes).

The fixtures for the `agent/integral` slice (placing `Integral` / `Fractional`
into `Numeric`'s type class hierarchy) use the prefix `ig` (`ig_hier` /
`ig_hier_bad`) and live in `crates/cli/tests/integral.rs` for the same reason.
`ig_hier.scala` gathers `List.range` / `Vector.range` / `Seq.range`, **the class
name of the instance selected** for 13 `implicitly[…]` calls, `quot` / `rem` /
`div`, user code taking a `Numeric[T]` implicitly, `sum` / `product` / `sorted` /
`max` / `min` / `sortBy`, the widening `Integral[Int]` → `Numeric[Int]` /
`Ordering[Int]`, and `Ordering[Option[Int]]`, and runs under `java -Xverify:all`
in both the library dual run and an output diff against **real scalac 2.13.16**
(`ig_hier_matches_real_scalac`). Because it prints class names, what is visible is
not "it became unambiguous" but "**it selects the same instance as real
scalac**". `ambiguity_did_not_increase` pins that not a single `ambiguous` comes
out of `Ordering[Int/Double/Long/Byte/Short/Char/Float]` or `sum` / `product` /
`sorted` / `max` / `min` / `sorted` on tuples (with `Numeric[T] extends
Ordering[T]`, this was the most fragile spot in this change). `ig_hier_bad.scala`
pins that the hierarchy is not a rubber stamp — the reverse flows `Numeric[Int]`
→ `Integral[Int]` and `Ordering[Int]` → `Numeric[Int]`, and the nonexistent
`Integral[Double]` / `Fractional[Int]` / `Integral[String]` (real scalac also
reports 6 errors on the same 6 lines). The private runtime has no
`scala/math/Integral`, so `range_is_diagnosed_without_the_jar` checks that
`--no-scala-library` **properly diagnoses** `not found: type Integral` /
`range is not a member of List$`.

The fixtures for the `agent/ordsummon` slice (the `Ordering` companion in term
position and summoning `Ordering[T]`) use the prefix `os2` (`os2_summon` /
`os2_summon_bad`) and live in `crates/cli/tests/ordsummon.rs` for the same reason.
`os2_summon.scala` gathers `Ordering.Int.reverse` / `Ordering[String]` /
`Ordering[Int].reverse` / `Ordering.String.reverse` /
`implicitly[Ordering[Int]].reverse` / `List(…).sorted(Ordering[String].reverse)` /
`Ordering.by[(String, Int), Int]` / `Numeric[Int]` / `Numeric.IntIsIntegral` /
`Integral[Int]` / `Fractional[Double]` / `BigInt` multiplication / the class name
of the selected instance (`scala.math.Ordering$Int$`) /
`List(Some(2), None, Some(1)).sorted`, and runs under `java -Xverify:all` in both
the library dual run and an output diff against **real scalac 2.13.16**
(`os2_summon_matches_real_scalac`). The `ClassCastException` came out **after**
type checking, so compiling successfully is not enough on its own.
`the_three_reported_forms_run` runs the three reported shapes as they were, and
`integral_and_fractional_summon` pins the shape where `val i: Integral[Int] =
Integral[Int]` quietly compiled and failed at run time (a type error as of
`59d967a`). `option_ordering_is_still_derived_but_is_not_a_view` checks both that
`Ordering.Option` still works as a derivation rule and that it does not work as a
view (`val o: Ordering[Option[Int]] = Ordering.Int` is a `type mismatch`).
`module_apply_redirect_still_works` pins that the existing factories
`List[Int](1, 2)` / `Vector[String]` / `Option[Int]` / `Map[String, Int]` do not
become `ambiguous overload` (supplying `apply` from pickles was added here, so
this was the most fragile spot). `alias_module_keeps_the_pickled_overloads` pins
`BigDecimal(3L)` / `BigDecimal(BigInt(6))` / `BigInt("7")` — **the regression that
got this slice reverted once** (when an alias resolves to a module it does not go
through `widen_with_companion`, leaving only the 3 hand-written prelude
candidates). `oshadow` covers the same program end to end, but this one covers the
alias path itself. `os2_summon_bad.scala` is the 5 lines showing that allowing a
companion in term position does not mean "anything goes" — `val a: Ordering[Int] =
Ordering` / `val b: Ordering[Option[Int]] = Ordering.Int` / `Ordering.Foo` /
`Numeric.Int` / `Ordering[Object]` — where real scalac also reports 5 errors on
the same 5 lines. `summon_is_diagnosed_without_the_jar` checks that the
`not found: value Ordering` diagnostic remains under `--no-scala-library`.

The fixtures for the `agent/traitextends` slice (a trait extending a class,
`abstract override` / stackable traits) use the prefix `trex` (`trex_stack` /
`trex_inherit` / `trex_mixin_bad` / `trex_ungrounded_bad` / `trex_object_bad` /
`trex_ctorargs_bad` / `trex_absover_class_bad` / `trex_ownimpl_bad`) and live in
`crates/cli/tests/traitextends.rs` for the same reason. `trex_stack.scala`
gathers a trait extending a class that takes constructor arguments, a chain of
`abstract override`, two linearization orders that give different results
(`LOUD-please-woof` / `please-LOUD-woof`), and references to inherited members
from a trait body, and runs under `java -Xverify:all` in **both the private
runtime and `--scala-library`**. `expected/trex_stack.txt` and
`expected/trex_inherit.txt` are **real scalac 2.13.16's output verbatim**. Three
bytecode-side invariants are pinned as well: `trex_super_accessor_shape` (an
anonymous class's `Loud$$super$speak` becomes `invokespecial Main$Dog.speak`, the
same shape as scalac's `Main$$anon$1`),
`trex_inherited_superclass_reaches_the_class_file` (`class X extends Loud`
extends `Main$Animal` in the class file too), and
`trex_trait_interface_does_not_extend_its_superclass` (the trait's interface does
not extend the superclass, and the `T$class` body emits a `checkcast` before
reading an inherited member). All 6 bad cases are confirmed to be diagnosed **in
both modes** (`--no-scala-library` and `--scala-library`), with wording from real
scalac 2.13.16. `trex_mixin_bad` also checks that it is rejected for both a named
and an anonymous class, and **once per template** (no double report with the
header pass).

The fixtures for the `agent/localconv` slice (local `implicit val` /
`implicit def` / `implicit class` written in a method body, a block or a lambda
body not being visible to view search; see the "local-scope implicit conversions
(views)" section of "the implemented language subset") use the prefix `lc`
(`lc_param` / `lc_class` / `lc_conv` / `lc_shadow` / `lc_capture` /
`lc_outofscope_bad` / `lc_ambiguous_bad`) and live in
`crates/cli/tests/localconv.rs` for the same reason. `lc_param.scala` is the
untouched control group (a path that already worked before the fix): a local
`implicit val` filling a nested `def`'s implicit parameter. `lc_class.scala`
checks a local `implicit class` is found as an extension method from all three of
a method body, a nested `def` and a lambda body; `lc_conv.scala` checks a local
`implicit def` works both as a coercion in an assignment and as a source of
extension methods for a separately declared local class. `lc_shadow.scala` checks
that a local `implicit def i2s` **shadows** an outer one of the same name (the
same `inner5` as scalac; not ambiguous), and `lc_capture.scala` is the shape where
a local `implicit class` captures another local (`factor`) and is `new`ed through
the synthesised conversion method — another nested local `def` — hitting an
independent bug in `lambda_lift`'s free-variable analysis. All of them run under
`java -Xverify:all` in **both the private runtime and `--scala-library`**, and the
`expected/*.txt` are real scalac 2.13.16's stdout verbatim.
`lc_outofscope_bad.scala` (an `implicit class` written in a sibling method is not
visible — `value dbl is not a member of 3`) and `lc_ambiguous_bad.scala` (two
local `implicit def`s of the same specificity give the same `ambiguous implicit`
as scalac) are both pinned in both modes.

The fixtures for the `agent/parentcheck` slice (diagnosing unresolvable parent
classes/traits, self types and `new`) use the prefix `pc` (`pc_parents` /
`pc_extends_bad` / `pc_selfnew_bad` / `pc_qualified_bad`) and live in
`crates/cli/tests/parentcheck.rs` for the same reason. `pc_parents.scala` is the
**good case**, gathering parents with arguments, generic parents, `with` mixins,
self types, anonymous classes, qualified parents and parents through a type
alias, and runs under `java -Xverify:all` in **both the private runtime and
`--scala-library`** (`expected/pc_parents.txt` is real scalac 2.13.16's stdout
verbatim). It is the net that catches a rule that is too broad. The three bad
cases are checked to be rejected **in both modes**, and also to write no class
file at all when they are. The wording is real scalac 2.13.16's:
`pc_extends_bad` covers four shapes (the head of `extends`, a `with` term, the
head of an applied parent, and its type argument — the same 6 errors as scalac),
`pc_selfnew_bad` covers a self type, `new Missing` / `new Missing {}` / `new Obj`,
and `pc_qualified_bad` covers six qualified shapes
(`is not a member of object …` / `… of package …` / `not found: value …` /
`object … is not a member of package …`).
`pc_new_of_a_missing_type_is_not_a_missing_value` pins that `new Missing` does not
revert to `not found: value` (the wrong namespace).

The fixtures for the `agent/setapply` slice (a companion's `apply` being
installed twice — once from the hand-written prelude and once from the jar's
pickle) use the prefix `sa` (`sa_setapply` / `sa_setapply_bad`) and live in
`crates/cli/tests/setapply.rs` for the same reason. `sa_setapply.scala` gathers
`xs(tag)` on a `Repo` trait (forcing `SetOps.apply(String): Boolean` to be
completed through a member, the same shape as the original report) followed by
`Set(...)`, then the reverse order, then the same shapes for `Map` / `List` /
`Seq`, and runs under `java -Xverify:all` in both the **`--scala-library` dual
run** and an output diff against **real scalac 2.13.16**
(`real_scalac_dual_run_sa_setapply`) — the reinstalled symbols change what is
linked, so getting past the verifier is itself the check. The private runtime has
no `scala.collection` pickles (hence no room for a double install), so
`sa_setapply_without_the_library_is_diagnosed` pins that under
`--no-scala-library` `Set` **does not quietly pass** but is diagnosed as
`not found: type Set`. `sa_setapply_bad.scala` pins that what was fixed is not
"the name" but "the shape of the erased parameters": two genuine overloads with no
common parent (`Pick.apply` on a `Cx` implementing `Ax` / `Bx`) stay two, and if
nothing settles it, it is rejected as scalac does. The first version (which
swallowed candidates it could not find as `None`) broke `agent/oshadow`
(`BigDecimal(2)` becoming `ambiguous overload`) and `agent/uniteq` (missing
members on `scala.Enumeration`) in the post-merge whole-tree verification; the
second version (the same check, but returning the existing prelude symbol instead
of swallowing) fixed both. See the corresponding sections below for the details.
