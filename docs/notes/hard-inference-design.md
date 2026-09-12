# Hard inference roots: GADT bounds, lenient prototypes, lambda results, retracted Nothing

Slice `agent/hardinfer` (2026-09-12). Four roots that earlier slices stopped
on, each traced to one nsc mechanism and implemented as that mechanism.
Tests: `crates/cli/tests/hinf.rs`, fixtures `tests/fixtures/hinf_*.scala`.

## 1. GADT-style refinement in patterns

nsc: `Infer.inferTypedPattern` / `inferConstructorInstance` end in
`instantiateTypeVar`, which *narrows the bounds of the enclosing method's
type parameter* (`freeTypeParamsOfTerms`: abstract types owned by a term)
for the duration of the case (`Context.pushTypeBounds`, restored by
`CaseDef.restoreTypeBounds`). `def ev[T](e: E[T]): T = e match { case I(i)
=> i }` with `I extends E[Int]` has `T` at `Int..Int` in that case.

Implementation:

* `SymbolTable::gadt_bounds` -- a stack of `(param, lo, hi)`; `is_sub_type`
  reads it in both directions (`a <: T` through `lo`, `T <: b` through
  `hi`). Kept out of the symbol so erasure and member supply never see a
  bound that exists for one case. `check_select` reads a receiver's
  `gadt_hi` for member selection (`t + 1` on `t: T` selects `Int.+`).
* `check_pattern::refine_gadt_bounds` is called for a typed pattern and for
  a constructor pattern: when the pattern type does not conform to the
  scrutinee outright, the pattern's base type at the scrutinee's class is
  unified against the scrutinee for every method-owned parameter it
  mentions. The bound follows the parameter's variance in the scrutinee
  (invariant: both; covariant `E[+T]`: lower only; contravariant: upper
  only). A solution naming anything but the pattern's own skolems (typed
  pattern binders, or the pattern class's parameters when the scrutinee
  could not fill them), a wildcard or `Nothing` is discarded; so is one
  that contradicts the bounds in force (declared, or an enclosing case's).
* `type_match` truncates the stack when a case ends, and takes a rigid
  type-parameter `pt` as the match type (nsc's `ptOrLub` with a fully
  defined `pt`), since a branch conforms to it only under its own bounds.
* Erasure: a member of a primitive class selected on a receiver whose
  erased type is a reference now unboxes the receiver (`erasure.rs`,
  `primitive_class_type`). This was a pre-existing miscompilation for a
  declared `T <: Int` too (`def f[T <: Int](t: T) = t + 1` was a
  `VerifyError`).

Evidence: `scala/util/Sorting.scala` 8 errors gone; `hinf_gadt` runs
identically under scalac and scala-rs; `hinf_gadt_bad` rejected at the
same five lines by both (class parameters are not refined, no leak across
cases, contradicting inner refinement discarded).

## 2. Nested polymorphic argument solved with the outer call

nsc: `handlePolymorphicCall` types each argument against the *lenient*
prototype -- the formal with the variables `protoTypeArgs` settled from the
expected type substituted and every other variable replaced by
`WildcardType`. `override def map[B](f: A => B): CC[B] =
strictOptimizedMap(iterableFactory.newBuilder, f)` settles `C2 := CC[B]`
from the declared result, hands `newBuilder` the prototype `Builder[_,
CC[B]]`, and `newBuilder`'s own `A` is read out of the invariant `CC[A]`
-- the only place it can be read from, `Builder[-A, +To]` being
contravariant in its element. This is why the same shape with a covariant
`List` (`fill(List.newBuilder, "a")`) is *rejected* by scalac: the
prototype gives `A` only an upper bound and `solve` minimises it to
`Nothing`.

Implementation: `proto_arg_type` used to refuse a partially settled formal;
it now substitutes `Type::Wildcard` for the unsettled own parameters.
`check_apply` marks the argument as typed under a relaxed prototype
(`relaxed_pt_depth`), and two places stop taking a bare wildcard from the
expected type as a *solution*: `solve_undet_result` and the receiver
variable block in `type_apply_in` (cats' `VarianceConversions`:
`leftWiden(rightFunctor.widen(fac))` took `X := _`).

Evidence: the 13 `StrictOptimized{Iterable,Map,Seq,SortedMap}Ops` errors
gone; `hinf_nested` (the library shape with a user-defined invariant
factory, plus the cats shape) runs identically under both compilers;
`hinf_nested_bad` rejected at the same lines, including the covariant
`List.newBuilder` forms scalac rejects.

## 3. Lambda bodies under an undetermined result

nsc: `typedFunction` types the body against `WildcardType` when the
expected result is not fully defined, so `x => if (x > 1) 1L else 0` takes
the weak-conformance lub `Long`. scala-rs put `Any` there in three places
(`check_apply`'s element-receiver rewrite, the bare-parameter rewrite, and
`open_to_bounds` for both the structural and the `Function1[A, B]` class /
`Named` spellings); all now use `Wildcard`, which `type_function` already
reads as "no expected type". `open_to_bounds` gained
`relax_function_result` so every site relaxes a function result the same
way.

Evidence: `hinf_lambda` prints identical element classes under both
compilers (`List(long, long)` where it was `List(Integer, Long)`);
gitbucket `IssuesService.scala` lost two `is not a member of Any` errors.

## 4. Invariant-position Nothing

nsc: `adjustTypeArgs` retracts a `Nothing` solution unless the parameter
occurs covariantly in the result (`restpe.isWildcard ||
!varianceInType(restpe)(tparam).isPositive`); the variable stays in
`context.undetparams` for the expected type or the enclosing expression,
and mono-mode `instantiate` closes it at a definition.

Implementation: `nothing_solution_retracted` replaces the `tryBreakable`
name check in `check_apply`; a retracted variable leaks through the
existing `undet_tvars` machinery (`ConstArray.newBuilder()` + `+`), an
expected type that pins it to `Nothing` keeps it, one that pins it to
something else lets `add_expected_constraints` record that; `val` / `def`
with an inferred type close leftovers to their lower bound
(`close_leaked_undet`), so `val b = List.newBuilder` is a
`Builder[Nothing, List[Nothing]]` and `b += 1` is scalac's mismatch.

Evidence: `hinf_nothing` runs identically; `hinf_nothing_bad` rejected at
the same five lines by both.

## 3a. Joining sibling library classes under no expected type (run/t4658)

Fixing root 3 exposed a latent dependency: a class read from the library
jar gets its parents from the pickle *lazily* (`pickle_supply::
ensure_parents`), and `SymbolTable::lub` walks whatever parent list is
attached. Typing a lambda body against `Any` used to force those parents as
a side effect of `adapt`; with the body typed under no expected type,
`if (r.inclusive) NumericRange.inclusive(…) else NumericRange(…)` joined
`NumericRange.Inclusive[Int]` and `NumericRange.Exclusive[Int]` to
`AnyRef` (`value sum is not a member of AnyRef`). `join_branches`
(`check_infer.rs`) retries `lub_branches` after `ensure_join_parents` --
the branch classes' own parents, one level, once per class -- and only when
the first join already fell to `AnyRef`/`Any`. The narrowness matters:
attaching whole ancestor chains for every `if`/`match` branch made
gitbucket's implicit searches and linearisations walk hierarchies it never
completed before. Fixture `hinf_join` (the t4658 shapes for `Range`,
`NumericRange[Int|Long|BigInt]`, a `match`, and `Some`/`None`).

A separate, pre-existing weakness surfaced on the way: `lub(List[Int],
Vector[Int])` is `IterableOnce[Int]` where nsc gives a `Seq[Int]`-based
refinement (`.length` is not a member). Not touched here.

## Deferred (reproduced, not fixed here)

* `IterableFactory[CC].newBuilder` read from the jar is "not a member":
  the classfile signature is `Builder<A, CC>` (a raw type variable of kind
  `* -> *` applied to nothing) and the reader drops it. Only jar users see
  this; the library measure compiles `IterableFactory` from source. Repro:
  `def x(f: IterableFactory[List]) = f.newBuilder[Int]`.
* `Option.map` / `Try.map` / `Either.map` are prelude declarations spelled
  `A => Any` (`prelude_coll.rs` style), so their lambdas are still typed
  against `Any`: `Option(3).map(x => if (x > 1) 1.0 else 0)` is
  `Option[AnyVal]` (scalac: `Option[Double]`). Fixing it means giving those
  prelude members a real type parameter.
* A retracted variable in a *function-typed* result applied directly
  (`fn(fail())("z")` for `def fn[T](x: => T): T => String`) is not solved
  by the function-value application path (both before and after this
  slice it is rejected; scalac accepts). Assigning it first works.
* `mk(inv(fail())).get.or("chained")` is accepted (scalac rejects: the
  chain minimises to `Nothing` in nsc). Over-acceptance, contrived.
* `collect` on a user-defined collection is retyped as the *receiver's*
  class by the `collect` result heuristic in `check_apply`
  (`receiver_collection_root`), a `ClassCastException` at run time:
  `class L[A] extends Ops[A, List] { def collect[B](pf): List[B] }`,
  `new L(...).collect { ... }`. Pre-existing; not touched.
* `m(List.newBuilder, _.toString)` and `m(List.newBuilder, x => x)` (a
  lambda beside a covariant builder) are accepted where scalac rejects
  them, before and after this slice.
