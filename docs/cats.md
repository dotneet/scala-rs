# typelevel/cats

> **Measurement note (2026-09-05).** `tests/cats_measure.sh` passes
> `-no-specialization`. cats writes `import scala.{specialized => sp}` and
> annotates with `@sp`; scala-rs used to reject that annotation without the
> flag, and **a single parse error aborts the run before any file is
> typechecked**, so the count collapsed to the parse errors alone (71) and said
> nothing about type checking. *(Since stage 1 of
> [`docs/specialization.md`](specialization.md) the annotation is accepted, and
> the run reports the same number with the flag and without it — 907 at the
> time of writing — so the flag is no longer buying anything. It is still
> passed.)* The honest figure at `997884a` is **2929 errors / 151 files**, of
> which the kind-projector symptoms (`*` 388, `λ` 158, `α` 104) are still the
> largest group — those are a compiler plugin, and real scalac rejects them too
> without it.
>
> **Update (`agent/kindproj`).** That plugin's syntax now has a flag:
> `tests/cats_measure.sh -Ykind-projector` measures **1128 errors / 141
> files**. Without the flag the number is unchanged at 2929, and it has to
> stay that way — see
> [`-Ykind-projector`](#-ykind-projector-kind-projectors-syntax-behind-a-flag-agentkindproj)
> for why the default is off.


Where this compiler stands on [typelevel/cats](https://github.com/typelevel/cats),
the second real-world benchmark after slick. This is a survey, not a campaign:
the point is to have the number and the symptoms written down.

## The material

| | |
|---|---|
| Repository | `https://github.com/typelevel/cats` |
| Revision | **`32a50dcfad9d897459bb755c4b5a22b4c7bc745c`** (tag `v2.13.0`) |
| Modules | `kernel` and `core` (`cats-kernel`, `cats-core`) |
| Scala | 2.13.16 |
| Sources | 340 (95 kernel, 245 core), of which 16 are generated |

`v2.13.0` is pinned rather than `main` because its published jars
(`cats-kernel_2.13-2.13.0.jar`, `cats-core_2.13-2.13.0.jar`) are in the local
Coursier cache, so a single file can be compiled in isolation against the rest
of cats when a symptom needs narrowing down.

`kernel` and `core` are the whole dependency chain: `core` depends on `kernel`
and nothing else (`algebra` depends on `kernel` too, but `core` does not depend
on `algebra`). `laws`, `free`, `alleycats` and `tests` are further out.

### What sbt actually compiles

Read off `sbt "show coreJVM/Compile/unmanagedSourceDirectories"` and
`"show coreJVM/Compile/scalacOptions"`, not guessed:

* Source directories for 2.13 are `scala`, `scala-2` and `scala-2.13+`;
  `scala-2.12` and `scala-3` are not compiled.
* 16 sources are **generated** by sbt source generators
  (`project/KernelBoiler.scala`, `project/Boilerplate.scala`): 1 for kernel and
  15 for core, including `NTupleMonadInstances.scala`, the largest single file
  in the measurement. Measuring without them would ask for a source set real
  scalac never sees, so `tests/cats_measure.sh` has sbt write them once.
* Dependencies are `scala-library`, `scala-reflect` (Provided, for the one
  macro) and `scalac-compat-annotation_2.13`.
* **`-Xsource:3` is on** (sbt-typelevel's default; only the `algebra`
  subproject opts out with `scalacOptions -= "-Xsource:3"`). Same as slick.
* **Two compiler plugins are on: kind-projector 0.13.3 and
  better-monadic-for 0.3.1.** We have neither. The first one is the story of
  this whole page; the second only changes how a `for` comprehension is
  desugared and costs nothing here.

## The harness

```
CATS_LOG=<your own path> CATS_RUN=<your own path> tests/cats_measure.sh
```

Same shape as `tests/slick_measure.sh`: it rebuilds *this* checkout's
`target/release/scala-rs`, re-fetches the material at the pinned revision when
`/tmp` or the scratchpad has been wiped, and writes every path per invocation.
**`CATS_LOG` defaults to a shared file — always set it to a path of your own.**

* `CATS_MODULES=kernel` measures kernel alone; `CATS_MODULES=core` measures
  core alone against the published `cats-kernel` jar; the default is both from
  source.
* `CATS_EXCLUDE` holds files out. It defaults to `FunctionKMacros.scala`; see
  below.

## The numbers

Both from the merged tree, `kernel+core`, 339 files:

| | files | errors | files with errors | classes |
|---|---|---|---|---|
| At the start of this slice | 340 | 755 | 29 | 0 |
| After it | 339 | **3019** | **165** | 0 |

The count going **up** is the result. The 755 were all *parse* errors, and a
parse error stops the run before a single file is typed — the first number said
nothing at all about type checking. 174 of 339 files now typecheck clean.

`classes=0` throughout: codegen does not run while there are errors, so cats
produces no class files yet. Kernel alone is much closer: **84 errors in 23 of
95 files**.

### One file is held out

`core/src/main/scala-2/cats/arrow/FunctionKMacros.scala` matches trees with
quasiquote *patterns* (`case q"($param) => $trans[..$typeArgs]($arg)"`).
Interpolated-string patterns are not implemented at all — `case s"a$y"` is a
parse error too — and one unparseable file suppresses the diagnostics of the
other 339. It is excluded by default and counted separately. Note this is one
file out of 340, and it is a macro implementation.

## Breakdown by symptom

```
   672  not found: type            (388 `*`, 158 `λ`, 104 `α`, 8 `β`, 14 real)
   590  kinds of the type arguments (…) do not conform
   398  … overrides nothing
   369  type mismatch
   318  value … is not a member of …
   228  no matching overload
   179  incompatible type in overriding
    82  ambiguous implicit
    67  … does not take type parameters
    28  not found: value
    22  no implicit: could not find implicit
    17  could not optimize @tailrec annotated method
    …
  3019  total
```

### Almost all of it is one missing compiler plugin

Split the 166 files that have errors by whether any of their errors names a
kind-projector construct (`not found: type *` / `λ` / `α` / `β`, or a kind
conformance failure on a type written with `*`):

| | files | errors |
|---|---|---|
| kind-projector symptom present | **70** | **2514** (83%) |
| no kind-projector symptom | 96 | 505 |

and the second column understates it, because the cascades are counted in the
first column's files. `NTupleMonadInstances.scala`, the worst file at 234
errors, is `private[instances] class FlatMapNTuple2[A0](A0: Semigroup[A0])
extends FlatMap[(A0, *)]`: the parent does not resolve, so all 10 of its
`override def`s then "override nothing". That is the shape of most of the 398
`overrides nothing` and much of the `type mismatch` mass.

cats writes `λ[α => F[G[α]]]` 165 times in 33 files and `F[A, *]` many more
times than that. Real scalac without the plugin reports exactly what we do
(`not found: type λ`), so the diagnostic is right; the plugin is what is
missing.

### `kernel` on its own: 84 errors in 23 of 95 files

`CATS_MODULES=kernel`. Not one of them is a kind-projector symptom, which makes
this the honest picture of everything else:

```
    8  value _1 is not a member of (A0)
    6  no matching overload for (T, T)T with arguments (A, A)
    6  no matching overload for (T, T)Boolean with arguments (A, A)
    5  type mismatch; found: T  required: A
    5  type mismatch; found: Duration  required: FiniteDuration
    4  value #:: is not a member of LazyList[A]
    3  value apply is not a member of Unit
    3  no matching overload for constructor BigDecimal with arguments (BigDecimal, MathContext)
    3  auxiliary constructor must start with a call to this(...)
    2  class StaticAnnotation needs to be a trait to be mixed in
    …
```

The `(T, T)T with arguments (A, A)` family (14 of the 84) is one shape: a
method declared on a trait in terms of its own parameter `T`, called through a
subclass that renamed it to `A`. The worst files are `Eq.scala` (10),
`TupleInstances.scala` (8), `SortedMapInstances.scala` (8) and
`PartialOrder.scala` (8).

The next section takes that list apart. The `(T, T)T` family turned out to be
**31 errors, not 14**, and one root; the count above is the diagnostic's
wording, not the shape.

## `kernel` after the `agent/kernel` slice: 19 errors in 10 of 95 files

Measured on the merged tree with `CATS_MODULES=kernel tests/cats_measure.sh`.

| | files | errors | files with errors |
|---|---|---|---|
| Before | 95 | **84** | 23 |
| After | 95 | **19** | 10 |

Ten roots, each with a minimal reproduction real scalac 2.13.16 accepts.
`tests/fixtures/k1_kernel.scala` holds all ten and runs; its expected output is
nsc's, and `crates/cli/tests/kernel.rs` diffs both compilers' output.

| errors | root |
|---|---|
| 29 | A higher-kinded type parameter's bound was resolved in a scope its own parameters are not in |
| 8 | `Tuple1` was not in the prelude |
| 8 | `this(a)(b)` in an auxiliary constructor, and a constructor group read at the wrong type arguments |
| 4 | `supply_receiver_override` compared arity where it had to compare parameter types |
| 3 | A `{ case … }` literal was not expanded to a SAM's arity |
| 3 | `new BigDecimal(java.math.BigDecimal, java.math.MathContext)` did not exist |
| 3 | `immutable.BitSet` extended nothing |
| 3 | A class stubbed from a pickle kept `AnyRef` as its only parent |
| 2 | `scala.annotation.StaticAnnotation` was declared as a class |
| 2 | A hexadecimal literal was read as a positive `i64` |

Adding `Tuple1` needed one repair elsewhere: slick's generated `TupleSupport`
writes `new Tuple1(s(0))` where a `Product` is wanted, and the prelude linked
`Product` / `Serializable` onto `Tuple2` and up only. That is the whole of the
slick difference — `tests/slick_measure.sh` is back at `errors=0
files_with_errors=0 classes=1596` on 184 files.

Three of these are worth spelling out, because the diagnostic pointed
somewhere else in each case.

### The bound of `P[T] <: PartialOrder[T]` was a name standing for nothing

`abstract class PartialOrderFunctions[P[T] <: PartialOrder[T]]` declares
`def lteqv[A](x: A, y: A)(implicit ev: P[A]) = ev.lteqv(x, y)`, and 31 of the
84 errors were calls of that shape reporting the bound's own parameter back:
`no matching overload for (T, T)Boolean with arguments (A, A)`, or
`type mismatch; found: T  required: A`.

`widen_type_param` already substitutes an application's arguments into the
bound. What it had to substitute into was `PartialOrder[Type::Named { name:
"T" }]` — an unresolved name. `T` belongs to `P`, not to the class, so it is
not in the class scope `type_class` re-resolves the bounds in. The namer's
provisional pass, which runs inside `enter_tparams` where the inner parameters
*are* in scope, had it right; this pass overwrote the good answer with the
broken one. Seven lines reproduce it:

```scala
trait Eq0[T] { def eqv(x: T, y: T): Boolean; def self: T }
abstract class F[P[T] <: Eq0[T]] {
  def eqv[A](x: A, y: A)(implicit ev: P[A]): Boolean = ev.eqv(x, y)  // (T, T)Boolean … (A, A)
  def mk[A](implicit ev: P[A]): A = ev.self                          // found: T  required: A
}
```

### A class whose only clause is implicit has the constructor `()(implicit …)`

That is nsc's answer, not a guess — `new C(3)` on `class C(implicit x: Int)` is
`no arguments allowed for nullary constructor C: ()(implicit x: Int): C`. It is
why cats-kernel writes `extends SortedMapEq[K, V]()(V)` and
`private[instances] def this(V: Hash[V], O: Order[K], K: Hash[K]) = this()(V, K)`.

Eight errors came out of that, in three different wordings, and they were two
roots:

* `this(a)(b)` was two applications, so the second landed on the `Unit` that
  `this()` produces. `extends A(1)(2)` and `new A(1)(2)` were already
  flattened; the self-call was not, and the delegation test only looks one
  `Apply` deep, so the same line also reported `auxiliary constructor must
  start with a call to this(...)`.
* With **two or more** constructors, `resolve_overload` re-reads the group off
  its symbols, where they are written in the parent's type parameters while the
  arguments are in the subclass's — so nothing matched. With one alternative
  the clause `pick_ctor_at` had already instantiated is used as is, which is
  why `extends E[K, V]()(V)` worked until `E` grew a deprecated `def this`.

### `x.min(y)` on two `FiniteDuration`s depended on what had been read first

`FiniteDuration` declares `min(FiniteDuration): FiniteDuration` next to the
`min(Duration): Duration` it inherits. `supply_receiver_override` only asks the
pickle for the receiver's own declaration when the class file shows an **arity**
no candidate has, and these two have the same arity — so the inherited
alternative stood and `x.min(y)` was a `Duration`. It only misfired *after*
something else had completed `FiniteDuration`, which is why importing
`Duration` by name changed the answer:

```scala
import scala.concurrent.duration.{Duration, FiniteDuration}   // drop `Duration` and it compiles
object FD {
  def mn(x: FiniteDuration, y: FiniteDuration): FiniteDuration = x.min(y)
  def mx(x: FiniteDuration, y: FiniteDuration): FiniteDuration = x.max(y)   // found: Duration
}
```

The comparison is now on erased *parameter* descriptors, which still excludes
the covariant override the arity test was guarding against (`List.length` over
`Seq.length` has the same parameters) and excludes bridges outright.

## What was fixed in this slice

Four of these were ahead of the typer, and the fifth killed the process. All
five are plain Scala 2.13 with no plugin involved; `tests/fixtures/c4_lang.scala`
and `crates/cli/tests/cats4.rs` pin them, dual-run against real scalac.

1. **`$` is a letter in an identifier.** nsc's `Chars.isIdentifierStart`
   accepts it. cats checks in simulacrum's generated output, which writes
   `implicit ev$1: Defer[G]`; the lexer reported `unexpected character '$'`
   47 times in 13 files. Note the exception: inside a `s"…"` hole nsc scans the
   name with `Character.isUnicodeIdentifierPart`, which does **not** count `$`,
   so `s"$l$r"` is two holes. Missing that cost slick one error
   (`b"\($l${concatOperator.get}$r\)"`), which is why the fixture pins both.
2. **A type parameter may carry annotations.** `TypeParam ::= {Annotation}
   [`+` | `-`] …`. cats-kernel writes `trait Eq[@sp A]` on 26 traits and each
   one was a dozen-error parse cascade.
3. **`@tailrec` on a def nested in a method** was rejected as "neither private
   nor final so can be overridden". A local def is not a member of anything.
   cats writes one inside `tailRecM` 79 times.
4. **A package written out in an expression had no members.**
   `cats.kernel.instances.int.catsKernelStdOrderForInt` reported `value … is
   not a member of <notype>` 161 times: only the *import* path knew to look in
   the package object. The qualifier is now rewritten to that package object's
   module, because a package is not a value and the backend has to push a
   receiver — without the rewrite the JVM got a `BoxedUnit` and threw
   `IncompatibleClassChangeError`.
5. **Expanding an abstract type member's alias could not terminate.**
   `SymbolTable::expand_type_members` recursed until the 512MB stack ran out,
   and *all 244 cats-core sources produced no diagnostics at all* — only
   `fatal runtime error: stack overflow`. See the next section.

## Known gaps, with the smallest reproduction of each

### `Type::TypeMember` has no prefix (`tests/fixtures/c4_alias.scala`)

cats' `Representable#compose` builds an anonymous class that defines
`type Representation = (self.Representation, G.Representation)` while the trait
it extends declares `Representation` abstract. Expanding that right-hand side
looks the name up again, finds the anonymous class's own alias, and expands it
again. A cycle guard now stops at the second visit, so the compiler answers
instead of dying, but the two prefixes still collapse onto the same member and
the file is reported as a type mismatch. nsc keeps `self.Representation` and
`G.Representation` apart *by the prefix*; we cannot, because `TypeMember`
carries only a symbol.

Real scalac 2.13.16 accepts the fixture. 19 lines, no plugin syntax.

### A structural type lambda is a type constructor (fixed)

This was the wall any kind-projector work ran into. A **named** higher-kinded
alias worked; the **structural** form -- the one kind-projector expands to, and
the one cats writes by hand where the plugin is not available -- did not:

```scala
trait Fun[F[_]] { def map[A, B](fa: F[A])(f: A => B): F[B] }

type EitherL[a] = Either[String, a]
val ok:  Fun[EitherL] = ???                                    // accepted
val bad: Fun[({ type L[a] = Either[String, a] })#L] = ???      // "required: Fun[<none>.L]"
```

Both are accepted now (`agent/typelambda`). The projection itself was never the
problem -- it produced a `TypeMember` of the right kind all along. Two other
things were:

1. **Two spellings of the same lambda were never the same type.** Every written
   refinement allocates its own `TypeMember` symbol, and `dealias` deliberately
   leaves a higher-kinded alias folded, because its body only means anything
   once applied. So `Fun[EitherL]` and `Fun[({ type L[a] = … })#L]` compared two
   unrelated symbols. Conformance now eta-expands both sides -- applying them to
   one side's own parameters -- and compares the bodies, which is how nsc
   decides it after dealiasing. A class constructor counts as one side, so
   `Fun[List]` conforms to `Fun[({ type L[a] = List[a] })#L]`.

2. **A lambda that captures an enclosing type parameter could not be
   substituted into.** `implicit def readerMonad[R]:
   Monad[({ type L[X] = Reader[R, X] })#L]` keeps its body in the symbol table,
   so instantiating `R = Int` left the body reading `Reader[R, X]`. Captured
   parameters are now the member's *leading* parameters and the projection is
   handed out already applied to them, which makes it a partial application that
   ordinary argument substitution can reach into. `kind_arity` of a partial
   application already subtracts what is applied, so the arity the rest of the
   compiler sees is unchanged.

`tests/fixtures/tl_lambda.scala` pins the accepted forms (dual-run against real
scalac 2.13.16 in both the library-ABI and the private-runtime mode) and
`tl_lambda_bad.scala` pins the four errors scalac reports for lambdas that do
*not* match, so the body comparison cannot degenerate into accepting anything.

Two things fell out of it, both cats shapes rather than lambda syntax:

* `type Aux[M[_], F0[_]] = Parallel[M] { type F[x] = F0[x] }` -- a refinement
  that names a type constructor member -- now carries `F0` where before it
  carried a placeholder that was the same whatever `F0` was. Implicit
  unification descends into a refinement's declarations to match it, and a
  parameter that occurs *only* inside those declarations counts as
  undetermined, so the witness can pin it down (nsc's `Context.undetparams`).
  `parUnorderedSequence[T, M, F, A](ta: T[M[A]])(implicit P: Parallel.Aux[M, F])`
  names `F` nowhere else. Reduced to thirteen lines, the shape used to report
  `found: F0[A]  required: F[A]`, leaking the alias's own parameter.
* Diagnostics print a lambda the way nsc does (`Functor[[a]Box[a]]`, not
  `Functor[<none>.L]`), and a refinement's declarations print through the
  symbol table (`{ type L[a] = Box[a] }`, not `{ type L[_] = tmem#5125 }`).

**What is still missing is kind-projector's surface syntax**, `λ[α => F[G[α]]]`
and the `*` placeholder. That is a compiler plugin, not Scala; nsc without it
reports exactly what we report, so the rejection is correct, and the 2514
errors in the 70 files that name `*`, `λ` or `α` are unchanged by this. The
desugaring should sit behind a flag. (It now does: `-Ykind-projector`, below.)

Measured on `kernel+core`, 339 files, twice -- once at the branch point and
once on the merged result, because `main` moved twice underneath:

| base | before | after |
|---|---|---|
| `40816a0` (branch point) | 3016 errors, 165 files | **2987**, 165 files |
| `6394ac6` (merged) | 2956 errors, 151 files | **2927**, 151 files |

The same 29 errors either way, and the *set* of files with errors is identical
before and after: no file gained one. (The 3019/165 recorded above was measured
elsewhere; this tree measures 3016/165 for the same commit.)

### What cats-kernel still reports (19 errors, 10 files)

Each of these has a reproduction; none is a cascade of another.

* **`#::` on a `LazyList`** (4, `EnumerableCompat.scala`). `aa #:: loop(aa)` is
  `LazyList.toDeferrer(loop(aa)).#::(aa)`: an implicit conversion to a *value
  class* whose method takes a by-name argument, and nsc lowers the call to
  `LazyList$Deferrer$.$hash$colon$colon$extension`. Nothing of that is
  modelled.
* **SAM conversion where the expected type is not written at the conversion**
  (4, `Eq.scala` 66/133/148, `Hash.scala` 81, `Order.scala` 118). Two separate
  gaps:
  - `val b: Option[(Int, Int) => Boolean] = Some((x, y) => x == y)` fails, and
    so does the SAM version. The expected type is not solved through `Some`'s
    own type parameter before its argument is typed, so the literal's
    parameters have no types. **Not SAM-specific** — the plain function type
    fails the same way.
  - `scala.math.Equiv` / `Ordering` / `Hashing` are prelude-declared traits
    whose members carry no `ABSTRACT` flag, so `sam_sig` finds no single
    abstract method and the conversion is never attempted. Marking them
    abstract is a two-line change and *should not be made yet*: see the SAM
    codegen gap below, which would turn these compile errors into
    `AbstractMethodError` at run time.
* **An overloaded implicit method used as a value** (2, `Eq.scala` 265/282).
  `cats.kernel.instances.sortedMap.catsKernelStdHashForSortedMap[K, V]` has two
  alternatives, both with only implicit clauses; nsc picks the one whose
  implicits resolve. We report the overload itself: `found: <overload (Hash[K],
  Hash[V])Hash[SortedMap[K, V]] | …>  required: Hash[SortedMap[K, V]]`.
* **`Deadline(FiniteDuration(…))`** (1, `DeadlineInstances.scala`) and
  **`x - y` on `FiniteDuration`** (1). Order-dependent: once
  `Duration.Infinite` has been read, `FiniteDuration(2L, SECONDS)` no longer
  conforms to a `FiniteDuration` parameter (`new FiniteDuration(2L, SECONDS)`
  still does). Reproduction:

  ```scala
  import scala.concurrent.duration.{Duration, FiniteDuration, SECONDS}
  object FD8 {
    def durMin(x: FiniteDuration, y: FiniteDuration): FiniteDuration = x.min(y)
    def lowest: Duration = Duration.MinusInf          // drop this line and it compiles
    def m: FiniteDuration = durMin(FiniteDuration(2L, SECONDS), FiniteDuration(5L, SECONDS))
  }
  ```

  It predates this slice: disabling both of the slice's pickle-side changes
  leaves it exactly as it is.
* **`StaticMethods.combineNIterable(Vector.newBuilder[A], x, n)`** (2,
  `VectorInstances.scala`). `Builder[A, R]`'s `R` is not solved from a
  `ReusableBuilder[A, Vector[A]]` argument. Also order-dependent: it appears
  and disappears across otherwise unrelated changes.
* **`SortedSet.empty(ordering)`** (1) and **`x | y` on two `SortedSet`s** (1).
  `empty` is inherited from `EvidenceIterableFactory$Delegate` as
  `<A> CC empty(Ev)`, so the call has to go out as `(Object)Object`. Declaring
  it in the prelude compiles and then dies with `NoSuchMethodError`, because
  the backend only looks for a library method's real descriptor on the
  receiver's *own* class file. A stub that links to nothing is worse than the
  diagnostic, so this stays reported.
* **`as.reduceOption(combine)` on an `IterableOnce[A]`** (1,
  `Semigroup.scala`). cats supplies it with its own implicit value class in
  `compat.scalaVersionSpecific`, imported by wildcard; the conversion is not
  found.
* **`WrappedMutableMapBase`** (1): `Tuple2[K, V2]` where `Tuple2[K, V]` is
  wanted.

### SAM conversion emits a class with no mixin forwarders

Not a cats *error* — it type-checks — but it is a program that compiles and
then throws, which is worse, and cats leans on SAM everywhere:

```scala
trait Eq0[A] { def eqv(x: A, y: A): Boolean; def neqv(x: A, y: A): Boolean = !eqv(x, y) }
val l: Eq0[Int] = (x, y) => x == y
l.neqv(1, 2)   // java.lang.AbstractMethodError
```

This compiler puts a trait's concrete method bodies into mixin forwarders in
each implementing class rather than into JVM default methods, and the anonymous
class SAM conversion generates carries only the abstract method. An ordinary
`class C extends Eq0[Int] { … }` does get the forwarder, so the machinery
exists; the SAM path does not run it. It predates this slice.

### Others, in rough order of mass

* **Interpolated-string patterns** (`case q"…"`, `case s"a$y"`) are not
  implemented; the diagnostic is a 14-error parse cascade rather than one
  "unimplemented syntax".
* **`@sp` is read as `@specialized`, and neither is specialized.** cats writes
  `import scala.{specialized => sp}` and then `@sp`. The parser resolves the
  rename and records what the annotation selects (`docs/specialization.md`),
  so the alias no longer slips past anything — but the phase is still missing,
  so cats-kernel compiles as if unspecialized: no `$mc*$sp` members, and the
  classes we emit are not ABI-compatible with what nsc emits.
  `tests/spec_classfiles.sh` is the ledger for that gap.
* **cats' `Newtype` encoding** (`type Type[A] <: Base with Tag[A]`) —
  `value toSortedSet is not a member of Newtype.Type[A]`, 32 errors in
  `NonEmptySet.scala` and its neighbours.
* **`LazyList` / `Stream` cons** — `#::` is missing as both a value and an
  extractor (`stream.scala`, `lazyList.scala`).
* **`cats.evidence.As`** — 17 errors, all `no matching overload for (L[Z])L[A]
  with arguments (As[A, A])`: substituting a higher-kinded parameter in a
  Liskov-style witness.

## `-Ykind-projector`: kind-projector's syntax behind a flag (`agent/kindproj`)

Measured on `kernel+core`, 339 files, on the merged tree. The only difference
between the two rows is the flag:

| | files | errors | files with errors | classes |
|---|---|---|---|---|
| `tests/cats_measure.sh` | 339 | **2929** | 151 | 0 |
| `tests/cats_measure.sh -Ykind-projector` | 339 | **1128** | 141 | 0 |

**1801 errors, 61% of the total, were one missing compiler plugin.** The set of
files with errors is otherwise unchanged: eleven files lost all of theirs and
one gained its first (below). `classes=0` still, because codegen does not run
while there are errors.

### The flag

`-Ykind-projector` is **not an nsc flag**. kind-projector is a compiler plugin;
nsc without it rejects `Either[E, *]` and `λ[α => F[α]]` exactly as this
compiler does with the flag off, and that rejection is *correct*, so it stays
the default. Scala 3 has a flag of this name for its own compatible version of
the syntax, which is where the spelling comes from. `--help` says so, and
`crates/cli/tests/kindproj.rs` pins that `tests/fixtures/kp_lambda.scala` is
rejected without it in both library modes.

### What it does, read off the plugin

The desugaring is a syntactic pass over the type trees the parser has just
built (`crates/parser/src/parse/kindproj.rs`), which is where the plugin sits
too: its phase runs after `parser`, so `scalac -Xplugin:kind-projector…jar
-Xprint:kind-projector` prints exactly what it produces. Every rule below was
read off that output rather than guessed, and this compiler's `--parse` dump
now matches it tree for tree, down to the invented parameter names:

```text
Either[Int, *]           ~>  AnyRef { type Λ$[β$0$] = Either[Int, β$0$] }#Λ$
Tuple2[*, Double]        ~>  AnyRef { type Λ$[α$1$] = Tuple2[α$1$, Double] }#Λ$
Function2[-*, Long, +*]  ~>  AnyRef { type Λ$[-α$3$, +γ$4$] = Function2[α$3$, Long, γ$4$] }#Λ$
λ[(α, β) => Either[β, α]] ~> AnyRef { type Λ$[α, β] = Either[β, α] }#Λ$
```

Three things are easy to get wrong and were checked:

* **A `*` binds to the innermost enclosing type application, not the
  outermost.** `Either[Int, List[*]]` is `Either[Int, [a] => List[a]]`. Because
  the parser builds applications bottom up, rewriting each one as it is
  finished gets this for free.
* **A function type is an application of `FunctionN`**, so `A => *` is
  `[b] => A => b` and `* => *` is `[a, b] => a => b`. cats writes `E => *`
  seventeen times.
* **A shape the plugin does not recognise is left exactly as written.**
  `λ[Int]` and `λ[α => F[α], β]` come out of nsc as `not found: type λ`,
  because the plugin's rewriter passes them through. Reporting something of our
  own there would be a diagnostic nsc does not have, so `kp_lambda_bad.scala`
  pins that we say the same thing.

The generated names follow the plugin's as well — a Greek letter chosen by the
*position* of the placeholder in the application, plus a counter — so a
diagnostic reads the way nsc's does. `Functor[Box]` where
`Functor[Pair[String, *]]` is wanted reports
`required: Functor[[β$0$]Pair[String, β$0$]]` in both compilers.

The desugaring target is the structural type lambda `agent/typelambda` made
work; nothing new was needed in the typer for the syntax itself.

Covered: the `*` placeholder with and without variance (`+*`, `-*`), the
higher-kinded placeholder `*[_]`, parenthesised tuples (`(A0, *)`), function
types, `λ` and `Lambda` with one or more parameters, reordered and repeated
parameters, higher-kinded parameters (`λ[F[_] => …]`), and variance written
either backquoted (`` λ[`+α` => …] ``) or as an application (`λ[(-[A]) => …]`).
`tests/fixtures/kp_lambda.scala` runs all thirteen forms and its expected
output is what scalac 2.13.16 with kind-projector 0.13.3 prints; the e2e test
dual-runs it in both the library-ABI and the private-runtime mode.

**Not covered: the term-level `λ[F ~> G](f)`**, which builds a `FunctionK`
value. It appears once in cats' main sources, inside a scaladoc example, and it
is a different (expression) rewrite. Without it, `λ` in term position is
`not found: value λ`, which is honest.

### One name per lambda, and a cycle that predates this

Two bugs turned the first measured run into `errors=0 classes=0` — the shape
`.agent-brief.md` warns about, a stack overflow with no diagnostic at all.

1. The plugin names every lambda's member `Λ$` and tells two of them apart by
   symbol. `symbol::subst_refine_aliases` matches a refinement's member **by
   name**, so a lambda whose body mentioned another lambda substituted one into
   the other and never stopped. The name now carries the file and a counter.
2. That was not the whole of it. cats' `Representable#compose` builds an
   anonymous class declaring
   `type Representation = (self.Representation, G.Representation)` over a
   parent that declares `Representation` abstract, and a `TypeMember` here has
   no prefix to tell `self.` from `G.`, so both collapse onto the member being
   defined and its right-hand side reads `(Representation, Representation)`.
   `subst_refine_aliases` expanded that forever. It now carries the members
   whose right-hand side it is already inside and stops at the second visit,
   which is what `expand_type_members` already did for the same shape (see
   "`Type::TypeMember` has no prefix" above — this is the same root, reached by
   the other path). **Nothing about kind-projector caused it**: the flag only
   let those files typecheck far enough to reach it. The no-flag number is
   unchanged at 2929 with the guard in.

### What the remaining 1128 are

By file, worst first:

```
  116  data/NonEmptyLazyList.scala          53  instances/NTupleMonadInstances.scala
   46  data/Ior.scala                       40  instances/NTupleBitraverseInstances.scala
   40  data/EitherT.scala                   39  Parallel.scala
   37  data/IorT.scala                      35  data/NonEmptyMapImpl.scala
   33  instances/NTupleUnorderedFoldableInstances.scala
   32  data/NonEmptySet.scala               31  instances/stream.scala
```

By symptom:

```
  179  incompatible type in overriding type TypeClassType  (simulacrum's `AllOps`)
  135  no matching overload
   67  NonEmptyLazyList does not take type parameters      (the `Newtype` encoding)
   57  type mismatch
   21  value copy is not a member of …
   17  kinds of the type arguments … do not conform
   17  could not optimize @tailrec annotated method
   17  no implicit: could not find implicit value
   …
 1128  total
```

None of them names `*`, `λ` or `α` any more, and none is one of this pass's own
diagnostics. The two biggest are the ones already written up above: simulacrum's
generated `AllOps` refinement (`type TypeClassType`), and cats' `Newtype`
encoding (`type Type[A] <: Base with Tag[A]`), which is what
`NonEmptyLazyList does not take type parameters` is.

**One file gained its first error**,
`core/src/main/scala/cats/conversions/VarianceConversions.scala`:

```scala
Bifunctor[F].leftWiden(Bifunctor[F].rightFunctor.widen(fac))
// no matching overload for (F[X, A])F[X, B] with arguments (F[A, C])
```

`def rightFunctor[X]: Functor[F[X, *]]` used to be an error itself, so the call
site said nothing. Now the lambda resolves and `X` is simply not solved from
the argument: inference does not look through a type-lambda application. That
is the next thing in the way rather than a regression of the desugaring.

## What would reduce this the most

**The two encodings cats builds its own API out of.** With kind-projector
behind `-Ykind-projector`, 1128 errors are left in 141 files, and the top two
symptoms are both a single encoding each:

1. **simulacrum's `AllOps`** — 179 `incompatible type in overriding type
   TypeClassType`, in every `@typeclass`-generated file.
2. **cats' `Newtype`** (`type Type[A] <: Base with Tag[A]`) — 67
   `… does not take type parameters` plus the 116 in `NonEmptyLazyList.scala`
   that follow from it.

After those, inference through a type-lambda application (the
`VarianceConversions` shape above) and `Parallel`'s `Aux` witnesses are the
next masses. `kernel` alone — 19 errors in 10 of 95 files, no kind-projector
anywhere in it — is still the better target for a slice that wants to finish
something.

## simulacrum's `AllOps`: an inherited bound read at the wrong type parameters

**179 errors, 30 files, one root.** `tests/cats_measure.sh` goes from
**1108 errors / 139 files** to **929 errors / 109 files**; nothing else in the
log changes, and no new symptom appears.

cats does not use the `@typeclass` macro annotation — it ships the expansion as
source. Every type class gets an `Ops` trait and an `AllOps` trait that
restates the same abstract type member, narrowing its upper bound at each
level:

```scala
trait Ops[F[_], A] { type TypeClassType <: Functor[F] }
trait AllOps[F[_], A] extends Ops[F, A] with Invariant.AllOps[F, A] {
  type TypeClassType <: Functor[F]
}
```

We reported `incompatible type in overriding type TypeClassType:
AllOps.TypeClassType does not conform to <: Functor[F]` — the declaration
failing to conform to *itself*.

`check_type_member_kind_override` aligned the *type member's own* type
parameters (`type C[T] <: TypedType[T]` overridden by `type C[T] =
JdbcType[T]`) but never the **enclosing class's**. So the inherited bound stayed
`Functor[F_Ops]` while the child's read `Functor[F_AllOps]`: two different type
parameter symbols, and `is_sub_type` rightly said no. The check only ever
passed when the traits took no type parameters, which is why it survived this
long. One `subst_as_seen_from(&Type::ThisType(class_id), …)`, the same step
`override_check::base_type_at` already does for methods, fixes all 179.

It also makes the diagnostic right when the parent is applied at a concrete
argument: `trait Sub extends Ops[Box] { type T <: Functor[Cell] }` now reports
`does not conform to <: Functor[Box]` rather than naming the parent's `F`.

Nothing was loosened. `tests/fixtures/co_allops_bad.scala` pins the four shapes
nsc rejects — a widened upper bound, a parent applied at a different argument, a
narrowed lower bound, and an alias outside the inherited bound — and we reject
all four, in the same places nsc does.

## cats' `Newtype` encoding: a module and a `type` alias sharing one name

**91 errors, one file cleared entirely.** `tests/cats_measure.sh
-Ykind-projector` goes from **907 errors / 108 files** to **816 errors / 107
files**; nothing else in the log changes.

`object NonEmptyLazyList { type Type[+A] <: Base with Tag }` declares its
`Type` member directly; a *different* file's package object exports
`type NonEmptyLazyList[+A] = NonEmptyLazyList.Type[A]`. The object and the
alias share one spelling in two namespaces — ordinary Scala, not a plugin —
and three separate bugs came out of resolving it:

1. **`lookup_type` handed back a module and the real type-namespace symbol
   together, unfiltered.** Its own doc comment already said the module is
   only a fallback "when nothing in the type namespace carries that name",
   but the implementation returned the whole scope bucket once *either* kind
   was present, and the caller picked whichever came first by accident of
   insertion order. `Newtype[A]` (the alias) and `Newtype` (the module, kind
   arity 0) coexist in exactly the scope this bug needs, and picking the
   module is what "`NonEmptyLazyList` does not take type parameters" was —
   67 of these, all in `NonEmptyLazyList.scala` itself.
2. **`expose_unqualified`'s guard bailed out too early.** It exists to pull a
   package's members into scope on demand, but it stops as soon as *any*
   symbol already answers the name locally — and the namer had already
   forward-entered the *module* `NonEmptyLazyList` into that same file's own
   scope (so its own later definitions can refer to it), which satisfied the
   guard and stopped the alias from ever being looked up. Fixed by giving
   type-position exposure (`expose_unqualified_type`) its own guard,
   [`SymbolTable::has_real_type_entry`], that a module fallback cannot
   satisfy.
3. **The package-object member fold ran before cross-file parents were
   resolved.** `namer_module` folds a package object's members into its
   package as soon as the object's own body is namer'd, eagerly, so that
   `p.T` reaches a type an earlier file's package object declared. But
   `package object data extends ScalaVersionSpecificPackage` — the real cats
   shape, `NonEmptyLazyList`'s actual alias site — doesn't declare the alias
   in its *own* body; it inherits it from a parent class that may live in a
   file namer has not reached yet, so `rough_parents` (run in the same namer
   call) cannot resolve the parent, and the eager fold sees no inherited
   members at all. Fixed by recording `(package, package-object class)` pairs
   in `Typer::pending_pkg_folds` and redoing the fold with
   [`SymbolTable::members_including_inherited`] once the header pass has
   resolved every unit's parents for real (`typecheck_units_src`, right after
   the header-pass block).

A fourth, smaller instance of the same "module ranks ahead of the type
alias" mistake was in `type_owner_members` (used for a *qualified* `p.T`,
where `p` is a package or object written out — `nel.data.Widget[Int]` in the
fixture below): a `type` member already beat a same-named module when both
were declared on the *same* symbol (`new Outer.Inner()`'s class-over-module
disambiguation), but a `type` alias reaching the owner only through the
deferred fold above still lost to a module that was a *direct* member,
because `lookup_member` naturally returns direct members before folded ones
and nothing reordered them. Given a third tier — class, then `type`
member/param, then module/package — in that order.

Fixed together: `tests/fixtures/nel_newtype.scala` reduces the three-bug
shape to one file (`package nel { package data { ... } }` so the object and
the alias-bearing package object are still declared in different namer
scopes, dual-run against real scalac 2.13.16 in both the library-ABI and
private-runtime modes), and `nel_newtype_bad.scala` pins that the fix does
not loosen arity checking (`Widget[Int, String]` is still rejected, in nsc
and here).

### What is still missing: `Type::TypeMember` still has no prefix

None of the three fixes above touch **57** further errors (`value
toSortedSet`/`toSortedMap`/`reduce`/… `is not a member of Newtype.Type[A]`,
unchanged before and after — 32 of them in `NonEmptySet.scala` and its
neighbours, the rest in `NonEmptyMapImpl.scala` and `NonEmptyChainImpl.scala`,
which use the same encoding). These calls reach the newtype through an
**implicit conversion** (`implicit def catsNonEmptySetOps[A](value:
NonEmptySet[A]): NonEmptySetOps[A]`), not a direct member, and `object
NonEmptySetImpl extends Newtype` never narrows `Type` in its own body — it is
purely inherited from the shared `private[data] trait Newtype { type Type[A]
<: Base with Tag }`, unlike `NonEmptyLazyList`, which redeclares `type
Type[+A] <: Base with Tag` directly. Implicit search for a conversion out of
an abstract type looks at the type's companion scope, and a `Type::TypeMember`
here carries only the defining symbol (`Newtype`'s own `Type`), never the
*prefix* (`NonEmptySetImpl.type`) the source actually selected it through —
so the search looks in `Newtype`'s companion (there is none) instead of
`NonEmptySetImpl`'s, and reports the type by the trait's name, not the
object's, exactly like `value toSortedSet is not a member of Newtype.Type[A]`
prints. This is the same "`Type::TypeMember` has no prefix" gap already
recorded above for `Representable#compose`, reached by a different path;
fixing it needs the type carrying a prefix, not another namespace-resolution
patch, and is deliberately left for a slice that wants to take that on.
