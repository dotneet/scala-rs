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

### `Type::TypeMember` has no prefix: the implicit-scope half (fixed)

The three fixes above left **57** further errors untouched (`value
toSortedSet`/`toSortedMap`/`reduce`/… `is not a member of Newtype.Type[A]` --
32 of them in `NonEmptySet.scala` and its neighbours, the rest in
`NonEmptyMapImpl.scala` and `NonEmptyChainImpl.scala`, which use the same
encoding). These calls reach the newtype through an **implicit conversion**
(`implicit def catsNonEmptySetOps[A](value: NonEmptySet[A]): NonEmptySetOps[A]`),
not a direct member, and `object NonEmptySetImpl extends Newtype` never
narrows `Type` in its own body -- it is purely inherited from the shared
`private[data] trait Newtype { type Type[A] <: Base with Tag }`, unlike
`NonEmptyLazyList`, which redeclares `type Type[+A] <: Base with Tag`
directly. Implicit search for a conversion out of an abstract type looks at
the type's companion scope, and a `Type::TypeMember` here carries only the
defining symbol (`Newtype`'s own `Type`), never the *prefix*
(`NonEmptySetImpl.type`) the source actually selected it through -- so the
search looked in `Newtype`'s companion (there is none) instead of
`NonEmptySetImpl`'s, and reported the type by the trait's name, not the
object's, exactly like `value toSortedSet is not a member of Newtype.Type[A]`
printed.

**91 further errors.** `tests/cats_measure.sh -Ykind-projector` goes from
**816 errors / 107 files** to **758 errors / 106 files** (one file --
`syntax/set.scala` -- cleared entirely); the 58 `Newtype`/`Newtype2` "not a
member" errors this section is about are all gone, no other file's error set
grew, and the small remainder of the 91 is later diagnostics in files that
still had errors (the next wall right behind this one, e.g. `NonEmptySet.scala`
now reports `type mismatch; found: SortedSet[A]  required: Iterable[A]` where
it used to stop at the member lookup) -- real progress, not a wash.

The `Representable#compose` half of this gap (`self.Representation` and
`G.Representation` -- two *value* prefixes of the same defining symbol,
needing to stay distinct rather than gain a companion) is unrelated to
implicit search and still open; see
[the section above](#typetypemember-has-no-prefix-testsfixturesc4_aliasscala).

Fixing this *without* changing what `Type::TypeMember` carries turned out to
matter. The first attempt did give it a prefix -- wrapping the resolved type
in the same `Type::Refined` "as-seen-from view" `Checker::projected_class_type`
already uses to carry a prefix past `Type::Class` (which has no room for one
either): `Type::Refined { parents: vec![TypeMember(id), ModuleRef(owner)],
decls: vec![<asSeenFrom>] }`. `SymbolTable::is_sub_type`, `display_type` and
dealiasing all already unwrap that view to its bare first parent, so on paper
nothing downstream should have noticed. In practice `WidgetImpl.unwrap(value)`
-- a generic method call whose own parameter is the *same* abstract member,
written directly (`def unwrap[A](w: Type[A]): List[A]`) -- broke: inferring
`A` from a wrapped `value` no longer unified against `unwrap`'s bare
`Type[A]`, because the type-argument inference that checks a call's arguments
against a generic method's parameters compares structurally and never
consults `SymbolTable::as_seen_from_view` the way `is_sub_type` does. Wrapping
a value used in a hundred places to fix one caller's implicit search is too
wide a blast radius to carry silently.

The fix that shipped instead never changes the `Type` at all. `Typer` gets a
side table, `type_member_prefixes: RefCell<HashMap<u32, Vec<SymbolId>>>`,
keyed by a type member's own defining symbol; `Checker::with_prefix_if_type_member`
(hooked into `tree_to_type`'s `AppliedTypeTree` case, right where `p.T[args]`
resolves `p` through `qualified_type_owners`) records the module `p` denotes
there whenever `T` stays abstract, without touching the type it returns. The
implicit search's `collect_type_parts` (`crates/typer/src/implicits.rs`) is
the only reader: its new `Type::TypeMember` arm adds every module ever
recorded for that member as an extra implicit-scope part, alongside the
existing upper-bound class. Every other consumer of `Type::TypeMember` --
subtyping, display, dealiasing, erasure, unification -- sees exactly the type
it always did.

The trade-off is coarser than a real prefix: `Newtype`'s single shared `Type`
member means `NonEmptySetImpl`, `NonEmptyMapImpl` and `NonEmptyChainImpl` all
record themselves against the *same* key, so implicit search for any one of
them now also offers the other two's conversions as candidates. Harmless when
an offered candidate's own parameter type fails to unify, the same way an
unrelated implicit already in scope is harmless -- which is what actually
happens for cats' own code, and is why the corpus number above only ever goes
down. It is not a general soundness fix, though: two *independent* newtypes
sharing one un-overridden abstract member, whose conversions happen to add a
same-named method, can pick the wrong one and accept a program real scalac
rejects. (Confirmed with a two-newtype variant of the repro below --
`WidgetImpl`/`GadgetImpl` both `extends Newtype`, and a `Widget[Int]` calling
a `.toList` that only `GadgetOps` defines type-checks here and is rejected by
scalac 2.13.16 as "value toList is not a member of Widget[Int]". cats itself
does not hit this in the measured corpus.) Closing that needs the same real
prefix the abandoned `Type::Refined` attempt tried to carry, plus fixing
generic-method unification to read `as_seen_from_view` the way subtyping
does -- both still open, and now with a known dead end recorded so the next
attempt does not re-spend the same slice rediscovering it.

`tests/fixtures/tm_newtype.scala` reduces the shape to one file (`Newtype`
declared once, `WidgetImpl extends Newtype` without overriding `Type`, and
the alias reached through a package object that inherits it from a parent
class -- the same cross-scope indirection `nel_newtype.scala` uses -- so the
object and the alias-bearing package object are still declared in genuinely
different namer scopes), dual-run against real scalac 2.13.16 in both the
library-ABI and private-runtime modes; `tm_newtype_bad.scala` pins that the
fix does not loosen arity checking (`Widget[Int, String]` is still rejected,
in nsc and here).

## Tuples, type lambdas, and `@tailrec` eligibility

**133 errors, four roots.** `tests/cats_measure.sh -Ykind-projector` goes from
**752 errors / 103 files** to **619 / 92**, and no new symptom appears in the
log. The whole cluster lives in the four generated `NTuple*Instances` files
plus the ten `instances/{eq,order,show,…}.scala` copies of one `Defer` cache.

| errors | root |
|---:|---|
| 53 | `value copy is not a member of …` -- a `TupleN` was not a `case class` |
| 33 | `value _N is not a member of …` |
| 20 | `inferred type arguments … do not conform to … bounds [F <: Product]` |
| 17 | `could not optimize @tailrec annotated method` |

`tests/fixtures/tt_tuple.scala` and `tt_tailrec.scala` run all four, with
`tt_tuple_bad.scala` and `tt_tailrec_bad.scala` pinning the nine shapes nsc
still rejects; `crates/cli/tests/tupletailrec.rs` dual-runs each against
scalac 2.13.16, comparing the output for the positive fixtures and the
rejected *line numbers* for the negative ones.

### `TupleN` is a `case class`

`prelude_tuple.rs` built the tuple classes with their fields and their
`<init>`, but not the `CASE` flag, and `CASE` is what `try_rewrite_case_copy`
keys on. So `fab.copy(_1 = f(fab._1), _2 = g(fab._2))` -- `NTupleBifunctor`
and `NTupleMonadInstances` write 22 of these -- reported `value copy is not a
member of (Any, Any)`. Setting the flag reuses the rewrite that was already
there, including its re-inference of the class's type parameters, which is what
nsc's synthesized `copy[T1, T2]` does: `(1, "a").copy(_1 = "x")` really is a
`(String, String)`. Nothing else the flag reaches applies to a class the
prelude builds -- the `apply` / `unapply` / `Product` synthesis all runs off a
source `ClassDef` tree, and pattern matching already took the constructor arm
for anything with `ctor_fields`.

### A fully applied type lambda is its body

kind-projector's `(A0, *, *)` desugars to the structural type lambda
`({ type L[x, y] = (A0, x, y) })#L`, and `F[Any, Any]` at that argument is a
`Type::Applied` whose constructor is the lambda. `dealias` deliberately leaves
a higher-kinded alias folded -- its body means nothing until the arguments
arrive -- but here they *have* arrived, and nothing reduced it: `fa._2`
reported `value _2 is not a member of [A0, x, y](A0, x, y)[A0, Any, Any]`.

Two places now reduce a *fully* applied one (`expand_applied_hk_alias`, which
already existed for conformance): the receiver in `type_select`, so the member
is looked up in the body and substituted at the body's arguments; and
`class_sym_of`, so `.copy` finds the `Tuple3` to rebuild instead of chasing the
alias's upper bound. The diagnostic prints the reduced type as nsc's does.

### A higher-kinded parameter's bound is eta-expanded too

`private def instance[F[_, _] <: Product]` at `(A0, *, *)` was rejected:
`inferred type arguments [[x, y](A0, x, y)] do not conform to method
instance's type parameter bounds [F <: Product]`. nsc keeps a higher-kinded
parameter's bounds *inside* its `PolyType`, so `F[_, _] <: Product` reads
`[x, y]F[x, y] <: [x, y]Product` and `isPolySubType` decides it on the bodies.
Here the bound is stored as the written `Product`, so the eta-expansion has to
happen at the comparison: `hk_ctor_meets_proper_bound` applies the constructor
to its own parameters and asks about the result. It runs only after the plain
comparison has already said no, so it can widen what is accepted and never
narrow it.

### A method type parameter solved from a compound expected type

`collect_expected` -- the walk that reads a method's type parameters out of the
expected type -- had no arm for `Type::Refined`. cats'
`instance[F[_]](…): Traverse[F] with Reducible[F]`, assigned to a
`Traverse[Tuple1] with Reducible[Tuple1]`, therefore solved `F` from nothing at
all, the argument function's parameter came out as `F[Any]` with `F` still its
own placeholder, and `NTupleUnorderedFoldableInstances` reported `value _1 is
not a member of _[Any]` 22 times. Parents are now paired by the class they
name, so `Traverse[F]` is never read against `Reducible[…]`.

### `@tailrec` is `isEffectivelyFinalOrNotOverridden`, not "private or final"

nsc's `TailCalls` accepts a method that cannot be overridden, which is wider
than `final` / `private` / a member of an `object`. Confirmed against scalac
2.13.16, four more shapes are eligible:

* a member of the `$anon` class of a `new C { … }` -- nsc gives that class the
  FINAL flag. cats has 7;
* a member of a `sealed` class that no subclass overrides (nsc reads its
  `children`; here the symbol table is scanned, which costs nothing because
  the branch is only reached for a `@tailrec` method in a sealed or local
  class);
* a member of a class declared inside a block, which can only be extended from
  inside that block;
* a `def` in a `val`'s right-hand side. This one needed a new signal rather
  than a new rule: such a def is *owned by the enclosing class*, because there
  is no accessor symbol to own it, so nothing about the symbol distinguishes it
  from a real member. `Typer::block_local_defs` records the `DefDef`s that
  stand as statements of a block, before any of them is typed. cats has 10,
  one per `instances/{eq,equiv,function,hash,order,ordering,partialOrder,
  partialOrdering,show}.scala`.

**Widening this cannot change what a program does at run time.** scala-rs
implements no tail-call elimination at all -- `@tailrec` appears nowhere in
`crates/backend/` -- so a `@tailrec` method compiles to the same recursive
calls whether the annotation is accepted or rejected, and a deep enough
recursion overflows the stack either way. (Measured: a `final` method with two
million self-calls throws `StackOverflowError` from a scala-rs build both
before and after this change.) The check is a diagnostic, and the only thing at
stake is whether it matches nsc's.

## Monad transformers: an `if`/`match`-bodied lambda that decided nothing (`agent/monadtrans`)

**88 errors, from 752 to 664.** `tests/cats_measure.sh` goes from **752
errors / 103 files** to **664 errors / 101 files**. `EitherT.scala` 40 → 27,
`IorT.scala` 36 → 24, `OptionT.scala` 21 → 16, `Ior.scala` 46 → 4,
`NTupleUnorderedFoldableInstances.scala` 33 → 31, and nine smaller files
improve. No file gets worse; slick, gitbucket and the corpus are unchanged.

`EitherT`, `IorT` and `OptionT` are all the same shape -- wrap an `F[…]`, and
rebuild by handing `F.flatMap` a pattern-matching anonymous function:

```scala
def biflatMap[C, D](fa: A => EitherT[F, C, D], fb: B => EitherT[F, C, D])(implicit F: FlatMap[F]) =
  EitherT(F.flatMap(value) {
    case Left(a)  => fa(a).value
    case Right(b) => fb(b).value
  })
```

Every one of them reported `no matching overload for
(F[Either[A, B]])EitherT[F, A, B] with arguments (F[_])`. Four roots, all
reproducible in a dozen lines without cats (`tests/fixtures/mt_transformer.scala`):

1. **`pt_or_lub` adopted the undetermined stand-in.** An undetermined variable
   in the *result* of a function-typed parameter is opened to `Type::Wildcard`
   rather than to a bound (`check_apply`'s `relaxed`), so the literal is typed
   against `X => F[_]`. `F[_]` is not `Any`, so `pt_or_lub` took it as the
   `match`'s type, the lambda came out `X => F[_]`, and the argument that was
   supposed to *decide* `flatMap`'s second parameter said `B = _` instead. The
   same body written as a plain lambda (no `match`, no `if`) always worked --
   that asymmetry is what pinned it. Fixed by `Typer::branch_result_ty`.
2. **Branches that disagree.** `EitherT.orElse` gives `F[Either[C, BB]]` from
   one branch and `F[Right[C, BB]]` from the other. nsc never joins the two
   applications: the expected type is a real type variable, each branch adds a
   *lower bound*, and `solve` takes the lub of those. Joining the applications
   cannot reach the same answer -- `F` is an abstract constructor whose
   parameter is invariant -- and `SymbolTable::lub` has no arm for
   `Type::Applied` at all, so it walked out to `AnyRef`. `Typer::fill_undecided`
   fills the stand-in positions from the branches, one argument at a time,
   which is the same computation nsc's `solve` performs.
3. **`collect_expected` had no `Type::Refined` arm.** cats' generated
   `NTupleUnorderedFoldableInstances` calls `private def instance[F[_] <: Product]
   (…): Traverse[F] with Reducible[F]` with `F` named nowhere else, so only the
   expected type says what it is. All 22 tuple instances reported `value _1 is
   not a member of _[Any]` followed by `found: Traverse[F] with Reducible[F]`;
   both symptom groups are now zero. (What is left in that file is the *next*
   thing: `F` is now solved to a kind-projector type lambda, and
   `[A0, β](A0, β)[A0, Any]` is never beta-reduced, so `.copy` is not a member
   of it. Eleven errors, all one shape.)
4. **An extractor lined up with the scrutinee by position.** `unify_one` pairs
   two class applications argument-by-argument, which is right only for
   applications of the *same* class. cats writes `final case class Right[+B](b: B)
   extends (Nothing Ior B)`, whose synthesized `unapply[B](x: Right[B])` was
   unified straight against a scrutinee `Ior[A, B]` -- so `case Ior.Right(b)`
   bound `b: A`, and every `IorT` method that matches on its own value reported
   `type mismatch; found: A  required: B`. `Typer::align_for_unify` walks one
   side to the other's class first, in whichever direction exists.

A fifth root turned up in the same file and is the bulk of `Ior.scala`'s 46 →
4: **`Ior.Left(a)` was typed as `scala.util.Left`.** The `Left.apply` /
`Right.apply` shortcut keyed off the owner module's *name* being `Left$` and
then asked the scope for a class called `Left` -- the same
look-it-up-by-simple-name mistake `factory_result_class`'s comment records for
`mutable.Set`. `cats.data.Ior.Left("x")` therefore had type
`Left[String, Int]`, even written out in full. The class now comes from the
`apply`'s own declared result, and a one-parameter `Left` is left alone.

`tests/fixtures/mt_transformer.scala` reduces all five to one file, dual-run
against real scalac 2.13.16 (`--scala-library` only: it needs `Either`,
`Tuple1` and `Product with Serializable`); `mt_transformer_bad.scala` pins
that filling a stand-in from the branches is not "believe the branches" -- a
branch that is not an application of the same constructor still decides
nothing, an aligned extractor still binds one definite side, and a
one-parameter `Left` still does not conform to an `Either`. Tests in
`crates/cli/tests/monadtrans.rs`.

**Still open in this group** (13 of the 44 errors the slice was pointed at):
`IorT` 7, `OptionT` 3, `EitherT` 3. They are separate roots -- `M[Any]` from
a `Functor[M].map` whose element type collapses, an `F[_]` under
`tailRecM`, and a `$anon$.F` prefix that outlives its anonymous class -- not
a remainder of the four above.

## The collection cons operators and extractors (`agent/catstail`)

**53 errors, from 530 to 477.** `tests/cats_measure.sh` goes from **530 errors
/ 89 files** to **477 / 88**. `NonEmptyList.scala` 17 → 5, `stream.scala`
28 → 12, `lazyList.scala` 14 → 9, `arraySeq.scala` 11 → 9,
`NonEmptyLazyList.scala` 24 → 18. No file gets worse.

The wave's tail was flat -- the largest symptom was 10 -- so the work was to
re-bundle it by cause rather than by message. Four of the listed symptoms
turned out to be one area:

| symptom | count |
|---|---:|
| `not found: extractor +:` | 6 |
| `not found: extractor ::` | 5 |
| `not found: extractor #::` / `not found: value #::` | 8 |
| `value #:: is not a member of LazyList[A]` / `… of Stream[A]` | 10 |
| cascaded `not found: value tail / rest / a` | 11 |

`case h :: t` had always worked, because `scala.::` is a *case class* and the
pattern goes through the constructor-pattern path. Its three siblings are
plain objects with an `unapply`, and **none of them was in the symbol table**:

```text
scala/collection/package$$plus$colon$.unapply:(Lscala/collection/SeqOps;)Lscala/Option;
scala/collection/package$$colon$plus$.unapply:(Lscala/collection/SeqOps;)Lscala/Option;
scala/package$$hash$colon$colon$.unapply:(Lscala/collection/immutable/LazyList;)Lscala/Option;
scala/package$$hash$colon$colon$.unapply:(Lscala/collection/immutable/Stream;)Lscala/Option;
```

Five roots, each reproducible in a few lines:

1. **The extractor objects themselves** (`crates/typer/src/prelude_consextract.rs`).
   `+:` and `:+` are declared `unapply[A, C <: Seq[A]](t: C): Option[(A, C)]`
   rather than with scalac's `C with SeqOps[A, CC, C]`, which needs a compound
   type this symbol table cannot spell; the erased `SeqOps` descriptor is named
   in `gen_invoke.rs`, the same treatment the sequence factories'
   `unapplySeq` already had.

2. **A type parameter determined only through another one's bound.** Nothing in
   `unapply[A, C <: Seq[A]](t: C)`'s *parameter* mentions `A`, so
   `Typer::subst_unapply_tparams` left the head sub-pattern at an unresolved
   `A` and `h + 1` picked `String.+`. It now takes a second pass through the
   bounds of the parameters it did solve. Keeping `C` also matters:
   cats matches an `ArraySeq[A]` with `case _ +: rest` and hands `rest` to a
   method that takes an `ArraySeq[A]`. `align_for_unify` used to walk the
   scrutinee up to `class_sym_of(C)` -- which answers with the *bound's* class
   -- and bound `C = Seq[Int]`; a parameter that is itself a type parameter
   now takes the scrutinee whole.

3. **`scala.#::` is overloaded**, one alternative per lazy sequence type, and
   `find_unapply` took whichever came first: a `Stream` pattern bound its tail
   at `LazyList`. The scrutinee's own class now decides, with conformance as
   the tie-break, and a scrutinee that is neither is *rejected* -- nsc says
   "cannot resolve overloaded unapply" for `case h #:: t` on a `List`, and
   binding it to the wrong alternative would have emitted a call the JVM
   rejects. The `Stream` alternative and `Stream.Deferrer` are installed
   lazily, the first time source names a `Stream` in one of those positions:
   `Stream` has no hand-written prelude declaration, and stubbing it during the
   prelude is actively harmful, because `give_stub_its_kinds` leaves every
   `scala/*` symbol allocated before `prelude_end` alone -- the stub would keep
   zero type parameters for the whole run and `type Stream[+A] =
   scala.collection.immutable.Stream[A]` would stop converting.

4. **A method named `::` hid the case class from its own class body.**
   `type_ident` already had the rule (nsc's `typingConstructorPattern` mode:
   a non-stable method of the name does not qualify), but the *class* lookup in
   `check_pattern.rs` was a plain `lookup`, which stops at the innermost scope
   binding the name at all. cats' `NonEmptyList` declares
   `def ::[AA >: A](a: AA)`, and all five `case h :: t` in the file reported
   `not found: extractor ::`.

5. **A by-name implicit conversion was invisible to the view search.**
   `a #:: xs` is right-associative, so it selects `#::` on `xs`, which has no
   such member; what makes it work is
   `implicit def toDeferrer[A](l: => LazyList[A]): Deferrer[A]` plus a value
   class carrying `#::` / `#:::`. `conv_param_matches` compared the receiver
   against the parameter *as written*, so every conversion with a `=> T`
   parameter was skipped. The by-name-ness is the whole point -- neither the
   tail nor (for `LazyList`) the head may be forced -- and declaring it in that
   shape lets the ordinary machinery do the work: `adapt` builds the thunk, and
   the backend already routes a value class's members through
   `<owner>$.<name>$extension`. The one place the two libraries differ is that
   `Stream`'s `#::` takes its head strictly and `LazyList`'s does not.

`tests/fixtures/ct2_conscoll.scala` runs all of it (including an infinite
`ones = 1 #:: ones`, which is what says the tail is not forced),
`ct2_consshadow.scala` the `::` rule on both runtimes, and
`ct2_conscoll_bad.scala` pins the two rejections. Tests in
`crates/cli/tests/catstail.rs`.

**Still open in this area** (9 errors): `value :: is not a member of AnyRef`
(6) and `value +: is not a member of Any` (3) look like the same symptom and
are not -- they are lambda *parameter* inference, `ior.map(c :: _)` and
`(b, acc) => b +: acc`, where the expected function type of a generic method's
argument never reaches the placeholder. Separately, `ArraySeq(1, 2, 3)` and
`Stream(1, 2, 3)` do not resolve (`no matching overload for (Seq, Any)AnyRef`
/ `for Stream$`); the fixture builds those with `ArraySeq.unsafeWrapArray` and
`Stream.range` instead.

## Five roots in the flat tail (`agent/catstail3`)

**124 errors, from 474 to 350.** `tests/cats_measure.sh` goes from **474
errors / 88 files** to **350 / 81**. `Parallel.scala` 39 → 6,
`NTupleMonadInstances.scala` 21 → 11, `NonEmptySeq.scala` 15 → 7,
`NonEmptyVector.scala` 14 → 7, `NonEmptyLazyList.scala` 18 → 11,
`InjectK.scala` 3 → 0, `stream.scala` 12 → 7, `Chain.scala` 13 → 7, and
sixteen smaller files improve. No file gets worse. `scalalib_measure.sh`
drops 1647 → 1625 alongside; slick and gitbucket are unchanged.

The tail was flat -- the largest symptom was 15 -- so the work was to
re-bundle it by cause. Five roots covered fourteen of the listed symptoms, and
none of the bundles the wave's brief proposed survived contact: the
`NonEmptyParallel.F[A]` mismatches and the `M[A]` ones are the same root as
each other but have nothing to do with the `FlatMapTupleN` self-type, and the
four `Iterable[…]` / `SortedMap` / `Iterator` mismatches split two ways.

### An inserted `apply` never had its own type parameters solved

`cats.Parallel` reaches `FunctionK.apply[A](fa: F[A]): G[A]` through a
*value*:

```scala
trait NonEmptyParallel[M[_]] {
  type F[_]
  def sequential: F ~> M
  def parallel: M ~> F
}
…
val fta: P.F[T[A]] = Traverse[T].traverse(ta)(f.andThen(P.parallel.apply))(P.applicative)
P.sequential(fta)
```

`P.sequential` is a parameterless `def`, so the application path cannot
resolve a one-argument call against it, `insert_apply_on_nullary` rewrites the
callee to `sequential.apply`, and resolution is retried. That retry adapted the
arguments and then set the call's type to the alternative's *declared* result
-- no inference at all. `P.sequential(fta)` was therefore `M[A]` with `A` still
`apply`'s own type parameter, which is every `found: M[A] required: M[T[A]]`
and every `found: NonEmptyParallel.F[A] required: NonEmptyParallel.F[T[A]]` in
the file. `Typer::instantiate_inserted_apply` runs the same inference the main
path runs, twice: once before the arguments are typed, and once after, because
a function literal has no type on the first pass.

Thirteen lines reproduce it, and the abstract type member is essential -- with
`F` and `G` plain type parameters of the enclosing method the same call always
worked, because then the *receiver* is not a parameterless def.

### A parameterless collection member kept whichever `C` was asked for first

`tail` and `init` are declared `C`; `zipWithIndex` and `flatten` are `CC[B]`.
`check_apply` already rebuilds those around the receiver's own class
(`returns_receiver_collection` → `rebuild_from_receiver`), because neither the
prelude nor a pickle can spell `C`. A selection with **no argument list** never
reached that path.

That would have been harmless if member completion answered per receiver, but
it does not: `PickleSupply::complete` walks the queried class's linearization,
substitutes each step, and installs the result **on the class it was asked
about**. So the first `aSeq.tail` in a run puts `IterableOps.tail: C` on
`scala.collection.immutable.Seq` as `Seq[A]`, and every later `aVector.tail`
finds *that* by inheritance. The two-line file

```scala
def a[A](xs: Seq[A]): Seq[A] = xs.tail
def b[A](xs: Vector[A]): Vector[A] = xs.tail   // found: Seq[A]
```

fails, and compiles with the two lines swapped. This is the same hazard
`supply_receiver_override` documents for `Map#collect`, and its guard cannot
catch this case: it compares *erased parameters* only, deliberately, so two
nullary methods always look alike.

`Typer::rebuild_parameterless_collection` applies the existing rebuild on the
selection path under the same gates -- a `scala/collection/` class, a real
subclass of what the declaration named, `SeqView` exempt, and the widening
members refused for a sorted collection that would need an `Ordering`. For
`flatten`, whose only clause is implicit, it rebuilds the method type's result.

The `SortedMap`/`SortedSet` mismatches in `instances/sortedMap.scala` (9) are
*not* this: they are `needs_ordering_to_rebuild` declining on purpose, and
closing them means selecting `SortedMapOps`' own overload.

### cats' `compose` tower is overloading, not overriding

```scala
trait Functor[F[_]] extends Invariant[F] { def compose[G[_]: Functor]: Functor[λ[α => F[G[α]]]] = … }
trait Apply[F[_]] extends Functor[F]     { def compose[G[_]: Apply]:   Apply[λ[α => F[G[α]]]]   = … }
```

No `override` on either, and scalac is right to accept it: the implicit
parameter's type is different, and parameter types are invariant under
overriding, so these are two alternatives. `override_check`'s `same_type`
answers "same" for anything `robust` refuses to compare, and `robust` refuses
every type mentioning a type parameter -- so all nine of these (`Applicative`,
`Apply`, `Bitraverse`, `Contravariant`, `Distributive`, `Functor`,
`NonEmptyTraverse`, `Reducible`, `Traverse`) were "`override` modifier
required".

`definitely_different` settles the cases that need no argument at all: `C[X]`
and `D[X]` are the same type only if `C` and `D` are the same class, and one
being a **strict** subclass of the other says they are not. Strictness is what
rules out the duplicate symbols one class file read twice leaves behind -- two
readings of one class are mutual subtypes, and this says nothing about them.

### A constructor field and its accessor were two alternatives

`javap scala.Tuple1` shows both `public final T1 _1;` and `public T1 _1();`,
and `prelude_tuple` faithfully declares both for `Tuple1` and
`Tuple3`..`Tuple22`. Both were selection candidates, so `ff._1` came back as
`<overload (A) => B | (A) => B>`. Where the component is not a function that
survives -- `(1, 2, 3)._1` is fine -- but an application of the overload finds
no alternative at all, because `resolve_overload_with`'s `Type::Overload` arm
only collects `Type::Method` alternatives. cats' generated
`NTupleMonadInstances.scala` writes `ff._1(fa._1)` once per `FlatMapTupleN`:
ten reports. `Tuple2`, whose prelude declaration (in `prelude.rs`, not
`prelude_tuple.rs`) has the field alone, was never affected.

In Scala source the name is always the accessor -- nsc makes the field
`private[this]` -- so `drop_field_behind_accessor`, inside `drop_overridden`,
drops a `ctor_fields` symbol when the same owner also declares a nullary
method of that name and result type.

### Branches that each leave the *other* type parameter open

```scala
val lastIor = f(reversed.head) match {
  case Right(c) => Ior.right(NonEmptyList.one(c))   // Ior[?A, NEL[C]]
  case Left(b)  => Ior.left(NonEmptyList.one(b))    // Ior[NEL[B], ?B]
}
```

`lub_branches` closes each side's undetermined variables only to ask whether
the *other* side is then the join; here neither is, so both closings were
discarded and the ordinary walk joined an open variable against a real type in
both positions -- `Ior[AnyRef, AnyRef]`. Every later use of `lastIor` read
`value :: is not a member of AnyRef`, which is what
[`agent/catstail`](#the-collection-cons-operators-and-extractors-agentcatstail)
guessed was lambda *parameter* inference. It is not; it is the value's own
type. `NonEmptyList`, `NonEmptySeq` and `NonEmptyVector` have the same three
lines.

nsc minimises a variable with no upper constraint to its lower bound before
joining (`solvedTypes`), which is what `minimize_undet` already does on the
argument path. The minimised join is now taken when it **conforms to** the one
the ordinary walk found -- only ever a tighter upper bound, never a wider one.

`tests/fixtures/c3_parallel.scala` runs all five, byte-identical to real
scalac 2.13.16's output; `c3_parallel_bad.scala` and `c3_override_bad.scala`
pin the four rejections that must survive. Tests in
`crates/cli/tests/catstail3.rs`.

**Still the largest groups after this** (`EitherT` 25, `IorT` 23,
`NTupleMonadInstances` 11, `OptionT` 16, `Kleisli` 14): the monad-transformer
remainder the `agent/monadtrans` slice named as separate roots, plus the
kind-projector type lambda that is never beta-reduced (`[A0, β](A0, β)[A0,
Any]` has no `.copy`).


### Missing Type-Bound Checks for an Inserted `apply`

During validation of an existing slice, execution showed that bounds were not
checked before inferred type arguments were substituted. After
`def upper: UpperApply`, `upper("wrong")` was accepted even though it violates
`UpperApply.apply[A <: Number]`. The same false acceptance occurred for
`OwnerApply[T].apply[A <: T]` and `LowerApply[T].apply[A >: T]`, whose argument is
an invariant `Box[A]`. scala-rs compiled the source containing all three calls
to six class files, while scalac 2.13.16 rejected it.

The inserted `apply` now passes its receiver type through the same
`infer_method_tparams_in` path as an ordinary call, checks upper and lower
bounds with `check_tparam_bounds`, and only then substitutes the inferred
types. The newly built `Select` receiver is used because the original
parameterless method receiver carries a different `T`. `c3_bounds_bad.scala`
checks rejection of the three invalid calls, while `c3_parallel.scala`
compares a bounded valid call with real scalac.

## Two leftover type parameters, and the diagnostic that hid them (`agent/catseta`)

346 -> **326** errors, 81 -> 80 files. `IorT` 23 -> 17, `EitherT` 25 -> 19,
`OptionT` 16 -> 16 (its remainder is a different root).

The sharpest symptom was a mismatch whose two sides printed the same string:

```
error: type mismatch; found: IorT[F, A, B]  required: IorT[F, A, B]
 191 |     def apply[F[_], A](fa: F[A])(implicit F: Functor[F]): IorT[F, A, B] = IorT(F.map(fa)(Ior.left))
```

Two distinct symbols named `B`. Both roots are the same mistake -- a type
parameter nothing had instantiated yet was carried on as if it were a fixed
type, instead of being a variable for the enclosing call to solve.

1. **Eta-expansion in an argument position.** `map`'s own `B` is undetermined
   while its function argument is typed, so `Ior.left` is adapted at the
   expected type `A => _` (`check_apply`'s `relaxed`). That pins `Ior.left`'s
   `A` and says nothing about its `B`. `solve_eta_tparams` substituted what it
   could and left the rest standing; the leftover then travelled through
   `map`'s result into `IorT[F, A, B]`. Writing the same argument as an
   explicit lambda always compiled, which is what said the eta path was the
   difference.
2. **An inserted `apply` whose receiver is polymorphic.** `IorT.liftF(fb)` is
   `right(fb)`, and `def right[A]: RightPartiallyApplied[A]` takes no
   arguments at all, so its `A` is fixed by nothing until the `apply`'s result
   meets the declared type. `instantiate_inserted_apply` solved the *apply's*
   parameters; the receiver's were left standing. `liftF`, `liftK` and the
   `pure` of every `IorT`/`EitherT`/`OptionT` instance reported the same name
   against itself.

nsc eta-expands into an untyped `x$1 => Ior.left(x$1)` and types that, so the
unsolved parameters join the context's `undetparams` and the outer expected
type fixes them. `record_open_tparams` puts both cases on that footing:
`type_apply` already leaks a variable the result still mentions outward, and
`solve_undet_result` already solves it against `pt`.

Four things had to follow.

* **A variable is still bounded.** `solve_eta_tparams`, `instantiate_undet_arg`
  and `solve_undet_result` all turned a variable into a type with no bounds
  check, so `Box(Inv.make(a))` with `def make[A, B <: Number]` and a declared
  `Box[Inv[A, String]]` was accepted -- while the same call *without* the
  wrapper was rejected, because that path goes through `check_tparam_bounds`.
  `undet_solution_in_bounds` refuses the solution; the mismatch is then
  reported against what was written.
* **A missing implicit argument was being emitted as a missing argument.** The
  inserted-`apply` branch of `type_apply_in` returned without calling
  `fill_defaults_and_implicits`, so `IorT.liftF` compiled to an `invokestatic`
  with one operand too few -- a `VerifyError`, with the typer silent. Filling
  the clause turns six of those into `no implicit: could not find implicit
  value of type Applicative[F]`, which is why the count is 326 and not 320:
  the sites are real, and finding a `Monad[F]` for an `Applicative[F]`
  parameter inside `IorTMonad` is a separate gap.
* **An invariant parameter does not contain a wildcard on the left.**
  `is_sub_type` read `C[_ <: T]` and `C[T]` as containment in both directions.
  `Box[_ <: Unit]` is `Box[t] forSome { type t <: Unit }`, and an invariant
  `Box` admits it only if `t` *is* `Unit`; nsc rejects it with a note about
  the variance. Now so do we.
* **A by-name argument was never checked against its parameter.** `adapt`'s
  `ByName` arm wrapped whatever it was given in a thunk and returned, so
  `def >>[B](fb: => F[B])` accepted a `Box[_ <: Unit]` for `Box[Unit]`. The
  check that closes it is deliberately narrow -- only an argument whose type
  carries a wildcard, i.e. an existential a lub built. A by-name argument is
  adapted *before* the callee's own variables are finally solved, so the
  expected type there is often one nobody has committed to yet: checking it in
  general makes `run/transpose` fail, because `def wrap[T >: Null](body: => T)`
  asks for `List[List[Nothing]]` at every call site. Widening this needs the
  by-name argument to be adapted after the callee's inference, not before.

`tests/fixtures/ce_etainfer.scala` runs every shape and prints the values, so
an instantiation that merely compiles (`Nothing`, `Any`) cannot pass; its
output is byte-identical to scalac 2.13.16's. `ce_etainfer_bad.scala` and
`ce_etabound_bad.scala` pin the four rejections, both compilers.

The full scala/scala corpus is unchanged against
`tests/baselines/corpus-0d200adb.tsv`: 5324 rows, zero changed statuses.
`transpose` is the reason the by-name check is narrow -- the general form lost
it, and the narrow form does not.

### Naming the owner when two type parameters print alike

nsc writes `A(in method make)`. scala-rs now writes
`A (defined in method make)`, and only when the two sides of a mismatch would
otherwise be the same string, and only for the names that are actually
ambiguous there (`type_mismatch_message`). It is worth the twenty lines: it
turned three more `IorT` errors into self-describing ones the moment it
existed --

```
found: IorT[F, A (defined in method right), B]  required: IorT[F, A (defined in method liftK), B]
found: Vector[A (defined in method empty)]      required: Vector[A (defined in class Chain)]
```

-- and the second of those is in `Chain.scala`, which nobody had connected to
this cluster.

### The cost, measured

gitbucket went 895 -> **899**. All four are the invariant-wildcard rule
meeting an existential we should not have built:

```
acc.getOrElse(e._1, Set())            // acc: Map[A, Set[A]]
found: Tuple2[A, Set[_ <: A]]  required: Tuple2[A, Set[A]]
```

`getOrElse[V1 >: V](default: => V1)` should solve `V1 := Set[A]` and type
`Set()` at it, the way nsc does; we type the argument first, minimise its own
variable to `Nothing`, and lub `Set[A]` with `Set[Nothing]` under an invariant
`Set` -- which is exactly the existential the new rule refuses. The
conformance rule is nsc's (nsc rejects `Box[_ <: Unit]` for `Box[Unit]` in the
reduced case); the imprecision is upstream of it, in solving a
lower-bounded parameter from an argument that carries a variable of its own.
That is the next thing to fix here, and it is worth four gitbucket errors plus
whatever else the laxity was hiding.

### The remaining head

`type mismatch` is still the largest cluster (156 of 326), and no pair prints
identically any more except one that is not a type-parameter collision at all
(`Seq[A]` against `Seq[A]` in `NonEmptySeq.sortBy`, two different `Seq`
classes). The next families by size:

* the `$anon` refinement family the brief asked about is **not** this root and
  was left alone: `found: IorT[NonEmptyParallel.F, E, A] required:
  IorT[$anon$1780.F, E, A]`, 40 error lines mention an `$anon$` type. An
  abstract type member of an anonymous `Parallel` instance is not recognised
  as the same type as the trait's `F`. It needs its own reduction.
  (`agent/projection` later took ten of those lines -- the first-order ones,
  `Eval#flatMap` and `Representable` -- and confirmed the rest, all of them
  `NonEmptyParallel.F`, are the eta-expansion root above and not this one.
  See "Path-dependent type members" at the end of this file.)
* `Ordering[AA]` against `Ordering[A]` (6), `NonEmptyList[AnyRef]` against
  `NonEmptyList[C]` (4), `Map[K, B]` against `SortedMap[K, B]` (4).

### `andThen` / `compose`: three roots under one diagnostic

`ambiguous overload for andThen with arguments ((<notype>) => <notype>)` was
13 errors, `compose` 3 and `lazyZip` 4. The first two share nothing with the
third, and the first two are themselves two different roots. The two-line
reproduction needs no cats at all:

```scala
val pf: PartialFunction[Int, String] = { case 1 => "one" }
val g: PartialFunction[Int, Int] = pf.andThen(s => s.length)
```

**1. Specificity has to be strict.** `arg_score` deliberately lets a
one-parameter function type inhabit a `PartialFunction[A, B]` formal, so that
a `{ case … }` literal -- which reaches overload resolution as a plain
one-parameter function -- can be passed to `collect` or `recover`. That is the
*literal* being adapted, which is `typedFunction`'s job in nsc. Specificity
compares two declared signatures with `isCompatible`, which has no
function-to-`PartialFunction` coercion at all: `PartialFunction` declares two
abstract members, so it is not a SAM type either, and scalac 2.13.16 rejects

```scala
def f(p: PartialFunction[Int, Int]) = 0
val g: Int => Int = x => x
f(g)   // type mismatch; found: Int => Int  required: PartialFunction[Int,Int]
```

Scoring a match made `PartialFunction.andThen[C](k: B => C)` as specific as
`andThen[C](k: PartialFunction[B, C])` and the reverse, so nothing separated
them. The rule is now gated on `spec_probe`.

**2. The shape type was only being used for arity.** nsc's
`preSelectOverloaded` throws out alternatives with `Infer.shapeType`, and the
shape of `x => e` is `FunctionN[Any, …, Nothing]` while the shape of
`{ case … }` is `PartialFunction[Any, Nothing]`. Only the first rules out a
`PartialFunction` formal. That single difference decides which `andThen` runs,
and the two behave differently:

```scala
pf.andThen(s => s.length)     // B => C:            keeps pf's domain
pf.andThen { case "one" => 1 }// PartialFunction:   composes both domains
```

so getting it wrong is a wrong `isDefinedAt` at run time, not a compile error.
`narrow_by_lambda_shape` compared arities only; the argument trees are now
summarised as `ArgShape`s and handed to it. It is a *pre-selection*, so it
never narrows a set of one -- a plain literal against a lone `PartialFunction`
formal still adapts, as it does for scalac.

**3. `isInProperSubClassOf` was blind to a function-typed parent.**
`sealed abstract class AndThen[-T, +R] extends (T => R)` (`data/AndThen.scala`)
records `Type::Function` as its parent, and both `class_reaches` and
`base_type_instance` stop dead there. So the `override def andThen` /
`compose` it declares and the `Function1` members they override were two
equally specific alternatives with no owner relation to separate them.
`class_has_base` reads such a parent back as `FunctionN`; it asks about
symbols only, which is why it can carry no type arguments and still answer.

**`lazyZip` is a different root and was left alone.** Its two alternatives
have *identical* parameter types and unrelated owners:

```
lazyZip owner=Seq             :: (Iterable[B]) => LazyZip2[A, B, ArraySeq[A]]
lazyZip owner=AbstractIterable :: (Iterable[B]) => LazyZip2[A, B, Iterable]
```

That is one library member -- `IterableOps.lazyZip` -- adopted twice at
different instantiations. No specificity rule can separate them, because for
nsc there is nothing to separate: it sees a single member. This belongs to the
member-supply seam, not to `isAsSpecific`.

**Measured**, on `9739388f` alone (the fix's own effect, before merging main):
`tests/cats_measure.sh` 326 -> **303** errors, 80 -> 78 files. All 16
`andThen`/`compose` ambiguities are gone; the 4 `lazyZip` remain. gitbucket 867
(unchanged), the scala library 1554 -> 1553, slick unchanged at
`errors=0 classes=1490` with `MODE=b tests/slick_run.sh` at 12/12, 36/36.

All 1490 slick class files are **byte-identical** to the ones the same tree
without this change emits (`SLICK_OUT=… tests/slick_measure.sh` on both, then
`diff -r`). That is the check this change needs: slick had no errors either
way, so the only thing a wrong pick could have moved is the emitted call.

On the scala/scala corpus (`CORPUS_SIZE=full`): `losses=0`, and
`pos/existential-function-pt` newly passes -- it is this defect exactly:

```scala
def foo(a: Function[String, _ <: String]): a.type = a
foo(x => x)
def foo(a: PartialFunction[String, _ <: String]): a.type = a
foo({ case x => x })
```

## Path-dependent type members (`agent/projection`)

Ten error lines in three files: `Eval.scala` 102/103/106, `Representable.scala`
86, and the six in `Tuple2K.scala` 142-156. Measured 326 -> 316 at the branch
point (`20c39e49`) and 303 -> 293 against `main` at both `d056a7f7` and
`b4f0eb0f` -- the same ten each time, with nothing new. On the scala/scala
corpus
(`CORPUS_SIZE=full`): `losses=0`, with `pos/t8801` -- the Peano encoding whose
`type Prev <: Nat { type Succ = Nat.this.type }` is exactly this shape --
newly passing.

**The defect.** `Type::TypeMember` carries no prefix, so `p.T` and `q.T` were
the same type. `q.put(p.get)` type-checked -- nsc rejects it with
`found: p.T  required: q.T` -- and cats' `Eval#flatMap`, which writes

```scala
case c: Eval.FlatMap[A] =>
  new Eval.FlatMap[B] {
    type Start = c.Start
    val start: () => Eval[Start] = c.start
```

reported `found: () => Eval[FlatMap.Start]  required: () => Eval[$anon$1367.Start]`.
Both halves were wrong the same way: `c.start` came out at the *declaration*
in `FlatMap` rather than at `c`, and the anonymous class's `type Start =
c.Start` never expanded, because `type_member_as_seen` folded any alias whose
right-hand side was another type member back to itself -- a rule that is right
for a deferred member (`type A` is spelled as a self reference) and wrong for
an alias, and one `is_deferred_type_member` already draws correctly.

**The representation, and why not `Type::Projection`.** A path-dependent
member is a **symbol**: a deferred `TypeMember` allocated once per (path,
declaration) pair, owned by the path's last term so it prints as `p.T`, and
carrying the declaration's bounds read through the prefix
(`SymbolTable::path_member`). Every walk that already handles an abstract type
member handles this one with no new arm, and a walk that knows nothing about
paths is exactly as imprecise as it was before -- which is what a new `Type`
variant could not promise across conformance, substitution, as-seen-from,
erasure and the pickle at once.

Three places had to learn about it:

* **`is_sub_type`** relaxes in one direction only. A path member and the bare
  declaration it stands for conform both ways, because the bare declaration is
  still what this compiler produces everywhere the prefix is not tracked;
  when *both* sides carry a path they are compared as the distinct symbols
  they are. That asymmetry is the whole negative property.
* **`expand_type_members`** must not re-resolve a path member *by name* in the
  class it is expanding into -- that is what dropping the prefix looked like,
  and it is also what made an anonymous subclass's `type Rp = (self.Rp,
  other.Rp)` resolve its own right-hand side back to itself
  (`Representable#compose`; the `enter_alias` guard in `symbol.rs` exists for
  that shape).
* **Erasure and the pickle** both replace a path member by its declaration
  (`erase_ty`, `pickle_type`). They therefore agree with each other and with
  everything the backend saw before this change; the typer keeps the
  distinction and nothing downstream has to know it exists. Reading the
  *bound through the prefix* in erasure would have been a real divergence:
  `class C[X] { type T <: X }` bounds `c.T` by `String` for a `c: C[String]`
  where `T` itself is bounded only by `X`.

**Where a path is attached.** At a written type `p.T` (`path_dependent_type`),
at a member selected through a path (`receiver_path_members`, applied to the
*declaration* before the receiver's type arguments go in -- afterwards an
occurrence that came from a type argument is indistinguishable from one the
declaration wrote), and at a call whose result names one of the callee's own
parameters (`subst_dependent_paths`, nsc's dependent method types).

**Four restrictions, each with the measurement that asked for it.**

* **Only a member the prefix's own class still leaves deferred.** slick's
  `trait Node { type Self >: this.type <: Node }` is abstract at its
  declaration and fixed by every subclass, so `this2.Self` on a
  `this2: ParameterSwitch` is `ParameterSwitch`. Freezing it as a path member
  made six `:@` calls fail against their own declared result (slick 0 -> 22).
* **Only a first-order member.** A higher-kinded member (`type F[_]`) is a
  type constructor whose application drives inference and reduction; giving it
  a prefix as well is a separate step. cats' `Parallel` instances are written
  entirely in terms of `P.F` and the ones that fail there fail for the
  eta-expansion reason `agent/catseta` recorded, not this one.
* **Only a path that starts at a local** -- a parameter, a local `val`, a
  pattern binding -- **or at a class's self alias.** A path starting at an
  arbitrary member of a class carries an implied outer prefix
  (`backend.Session` inside `BasicProfile` is `this.backend.Session`), and
  reading it through another receiver has to *compose* the two: slick writes
  `profile.runSynchronousQuery(...)(s: profile.backend.Session)` and got
  `backend.Session` for the parameter. Composition is a step beyond this
  slice. A self alias is exempt because it is another spelling of `this` and
  so has no outer prefix either -- and it is what keeps
  `Representable#compose` correct.
* **Dependent substitution needs the arguments to line up one for one with
  the parameters.** Without it, `DurationConversions`' fourteen forwarders
  (`def nanos[C](c: C)(implicit ev: Classifier[C]): ev.R = nanoseconds(c)`)
  each failed against their own signature with `found: ev.R  required: ev.R`
  -- two different `ev`s (scala library 1554 -> 1567). It runs on a *later
  parameter clause* as well as on the result: `def foo(x: Int)(y: C)(z: y.T)`
  (`pos/t1569`) and `def lazyDep(t: T)(u: => t.U)` (`run/t6443-by-name` and
  `-varargs`) are the corpus's three tests for that half, and they were the
  only losses the first full corpus run reported.

**Not fixed here, and not the same mechanism**: a projection out of an
*abstract type* (`E#TableElementType`, `Backend#Session`). Taken by
`agent/absproj`, which represents it the same way this slice represents a path
member -- a deferred `TypeMember` symbol per (prefix, declaration) pair, not a
`Type` variant -- but settles it differently: a path member never reduces,
while an abstract projection reduces the moment its prefix is *instantiated*
(`SymbolTable::subst_projections`). The two meet in `Typer::at_term_path`,
where a projection reached through a term path (`def get[P <: Phase](p: P):
Option[p.State]`) is unwrapped back to its declaration and re-attached as a
path member: the term settles what the type does not. See
`docs/gitbucket.md`, "A projection out of an abstract type".

## The inherited self type read at the wrong arguments (`agent/selftype`)

**303 -> 291 errors, 78 -> 77 files.** Twelve `illegal inheritance` errors, all
of one root, all in generated code:

```
error: illegal inheritance: self-type FlatMapTuple1 does not conform to FlatMap[F]
 --> cats/instances/NTupleMonadInstances.scala:219:20
```

`class FlatMapTuple1 extends FlatMap[Tuple1]` inherits `self: FlatMap[F] =>`
from `FlatMapArityFunctions[F]`, by way of `trait FlatMap[F[_]] extends
Apply[F] with FlatMapArityFunctions[F]`. Read with `F := Tuple1` the
requirement is `FlatMap[Tuple1]`, which the class obviously satisfies. We were
comparing against the *unsubstituted* `FlatMap[F]`.

### Why it would not reduce

The substitution was being attempted, so the shape alone does not reproduce it.
A hand-written hierarchy of exactly this form compiles cleanly, in one file or
several, which cost an earlier attempt an afternoon. The trigger is a **three-way
declaration order**:

1. the trait that *declares* the self type (`FlatMapArityFunctions`) is typed
   **before** the class, so its `self_type` is already bound and the check has
   something to test;
2. the class itself;
3. the trait that *supplies the argument* (`FlatMap`) is typed **after** the
   class.

Only then does `check_self_conformance` run against a `FlatMap` whose own
parents are still the unapplied `Type::Class { args: [] }` that the header pass
installs -- `[Apply, FlatMapArityFunctions]` rather than `[Apply[F],
FlatMapArityFunctions[F]]`. With no arguments to substitute, `F` stays `F`.

In cats that order falls out of the file names alone. `tests/cats_measure.sh`
sorts the source set, the generated `FlatMapArityFunctions.scala` sorts before
`instances/NTupleMonadInstances.scala`, and the hand-written
`src/main/scala/cats/FlatMap.scala` sorts after both.

`tests/fixtures/selftype_inherited_hk.scala` reproduces it in a single file by
writing the three declarations in that order; move `FlatMap` above the class and
the fixture stops proving anything.

### What changed

Two things in `Checker::check_self_conformance`, `crates/typer/src/check_template.rs`:

* **The check is held back to the body pass.** During `sigs_only` the base-type
  chain is still half-built, so any conformance verdict it reaches is about a
  type that does not exist yet -- the same reason `inherited_superclass` is
  already held back. Nothing is lost by waiting: the body pass types every unit
  the signature pass typed and reaches the check for each of them.
* **The walk now carries each base type applied, not just its symbol.** The old
  worklist held bare `SymbolId`s and asked `subst_as_seen_from` to rediscover
  the instantiation from scratch. That found only one path into a trait
  inherited twice at different arguments, and deduplicated the other away:
  `class WrongArg extends FlatMap[Cup] with FlatMapArity[Box]` reached
  `FlatMapArity` as `[Cup]`, concluded `WrongArg <: FlatMap[Cup]`, and accepted
  a class real scalac rejects. Substituting along the walk instead fixes both
  the verdict and the message, which now names scalac's `FlatMap[Box]` rather
  than a raw `FlatMap[F]`.

Substituting along the walk makes the old `subst_as_seen_from` call redundant,
and running it as well substituted a second time over an already-instantiated
type: cats' `Nested` self types came out as the doubly-applied
`Apply[[α][F, G, α]F[G[α]][F, G, G[α]]]` and picked up eight new errors. It is
gone; `expand_type_members` still runs, and still carries the slick cake cases
the original comment describes.

`tests/fixtures/selftype_inherited_hk_bad.scala` is the negative half. Both of
its classes genuinely fail an inherited self type and real scalac 2.13.16
rejects both, at lines 20 and 25, against the substituted `FlatMap[Box]`; the
e2e test pins the lines and the types, not just the words `illegal inheritance`.

### The head after this slice

The twelve were a whole family, and removing them does not change the shape of
what is left: 291 errors in 77 files, still **149 `type mismatch`** and **84 `no
matching overload`**. Those two are *not* one root each -- 149 type mismatches
carry 116 distinct found/required pairs -- but they are not fifty either.

The largest single-root family left is **`Parallel` / `NonEmptyParallel`'s
abstract type member `type F[_]` read through an instance prefix**. Cats writes

```scala
trait NonEmptyParallel[M[_]] extends Serializable { type F[_]; def apply: Apply[F]; ... }
```

and every instance refines `F`. Reading `P.F` where `P` is a particular instance
has to give that instance's refinement; we produce the declaration's own
unrefined member instead, so the two sides print almost identically and compare
unequal:

```
found:    Applicative[[γ$24$]Nested[NonEmptyParallel.F, [β$5$]Validated[E, β$5$], γ$24$]]
required: Applicative[[γ$29$]Nested[$anon$1667.F, [β$28$]Validated[E, β$28$], γ$29$]]
```

**74 of the 291 error lines (25%)** name such a prefix-read type member, 33 of
them `NonEmptyParallel.F` specifically. They concentrate in exactly the files at
the head of the per-file count -- `EitherT` (19 errors in the file), `IorT` (17),
`OptionT` (16), `Kleisli` (14), `WriterT` (7) and `Parallel.scala` itself -- and
they are spread across all three of the surviving message kinds, which is why
clustering by message kind hides them.

This is path-dependent type member territory, so it belongs to whatever slice
owns that area rather than to a second self-type pass. It is the one place in
the remaining tail where a single fix should be worth tens of errors.

## The same member, higher-kinded (`agent/hkpath`)

**281 -> 252 errors, 75 files unchanged.** 28 error *locations* disappeared and
none appeared. `agent/projection`'s limit #2 -- "only a first-order member" --
is gone, and the dependent-method-type substitution that limit was protecting
had to grow two things before it could hold.

### Lifting the restriction, on its own

`can_be_path_member` refused any declaration with type parameters. Dropping
that one clause takes cats from 281 to **255**, and adds three errors of a
single new shape in two files (`files_with_errors` 75 -> 76):

```
found:    Parallel[[γ$20$]EitherT[M, E, γ$20$]] { type F[x] = […]Nested[P.F, […]Validated[E, …], …][E, x] }
required: Parallel[[γ$4$] EitherT[M, E, γ$4$]]  { type F[x] = […]Nested[P.F, […]Validated[E, …], …][E, x] }
```

-- the two sides printing *identically*, because they are two different `P`s:

```scala
def catsDataParallelForEitherTWithParallelEffect[M[_], E: Semigroup](implicit
  P: Parallel[M]
): Parallel.Aux[EitherT[M, E, *], Nested[P.F, Validated[E, *], *]] =
  accumulatingParallel[M, E]
```

`accumulatingParallel`'s result names `accumulatingParallel`'s own `P`. This is
exactly nsc's dependent method types and exactly what `subst_dependent_paths`
is for -- it just could not reach this call.

### Two reasons it could not

**1. The clause is one nobody wrote.** `accumulatingParallel[M, E]` is a
`TypeApply` with no argument list at all; the implicit clause is filled in by
`adapt_implicit_apply` (`check_infer.rs`), which builds the argument trees and
then hands out the declared result untouched. `subst_dependent_paths` now runs
there, on the parameters and the arguments that pass just went and found. The
scala/scala corpus has this shape twice: `pos/t10714` and `pos/t10714b`

```scala
class Bar { type Baz = Foo; def foo(implicit foo: Baz): foo.Out = ??? }
(new Bar).foo.foo
```

both fail on `ab18fc50` and both pass now.

**2. Every occurrence to substitute was inside a type lambda.** A type lambda
is not a shape in `Type`: `Nested[P.F, Validated[E, *], *]` is an *anonymous
alias symbol* whose body is stored beside it in the symbol table
(`refinement_type_member`, and kind-projector's `*` desugars to exactly that
refinement). So `mentions_path_member` said "no" about a type that displays
`P.F` twice, and `map_type` had nothing to rewrite.

`mentions_path_member_deep`, `path_members_in` and `subst_path_member_deep`
walk into such a body. A rewrite there cannot edit the alias in place -- the
same symbol stands for every use of that written type -- so it allocates a
copy, memoised on (alias, member, replacement) in `path_member_lambdas`, which
is what keeps two spellings of one substituted type comparing equal. Only
*anonymous* aliases are followed: a class's own `type X = …` is resolved by
name elsewhere, and cloning it would make two members where the source
declares one.

With both halves, cats is **252**, `files_with_errors` back to 75, and
`pos/t7753` (`indirect(converted)(23)` on `def indirect(si: SI)(v:
si.instance.Out)`) passes as well.

**`mentions_path_member` itself stayed shallow, deliberately.** `is_sub_type`
pairs it with `drop_path_members`, which takes `&self` and therefore cannot
rewrite a lambda body either. Answering "yes" for an occurrence its partner
cannot then remove makes `is_sub_type` recurse on an unchanged type: the first
build of the deep version overflowed the stack on cats' first `Parallel`
instance.

### Correctness

`tests/fixtures/hkp_member.scala` is `NonEmptyParallel` cut down to what the
defect needs, and it *runs*: `expected/hkp_member.txt` is what real scalac
2.13.16 prints for the same source, and the e2e test asserts scalac's own run
against that file as well. On `ab18fc50` the fixture does not compile at all --
seven errors, with the cats signature (`found: Cell[Par.F[A]] required:
Cell[Cell[Cell[Cell[Cell[$anon$2.F[A]]]]]]`, the declaration on one side and
the anonymous class's own member on the other, plus the alias folding back on
itself).

`tests/fixtures/hkp_member_bad.scala` is the negative half, and it is the half
that says the representation still means something:

```scala
def mix[M[_], N[_]](p: Par[M], q: Par[N])(fa: p.F[Int]): q.F[Int] = fa
def mixBack[M[_], N[_]](p: Par[M], q: Par[N])(fa: q.F[Int]): p.F[Int] = fa
def absent[M[_]](p: Par[M])(fa: p.G[Int]): Int = 0
```

`ab18fc50` **accepts the first two** -- `p.F` and `q.F` were both the
declaration `Par.F` -- and rejects only the third. Real scalac 2.13.16 rejects
all three, at lines 19, 22 and 25, and so do we now, with the same types; the
e2e test pins the lines and the types on both compilers.

### What it cost elsewhere

gitbucket 679/102, the scala library 1550/168 and slick `errors=0
classes=1490` are all unchanged, `MODE=b tests/slick_run.sh` is 12/12 36/36,
`tests/slick_subset.sh` is 184 files / 1490 classes / `verified=1490 failed=0`
/ `lint_problems=0`, and `tests/verify_all.sh` over those 1490 classes reports
`verify_failures=0`. On the corpus (`CORPUS_SIZE=full`), `losses=0`.

**Two of slick's 1490 class files are not byte-identical**, and both differ
*only* in their `ScalaSignature`: `javap -p -c` is identical for
`RelationalProfile` and `RelationalProfile$RelationalAPI`, and the pickle grows
by 199 bytes. The cause is in the source:

```scala
trait RelationalProfile … { self =>
  trait API … { type ColumnType[T] = self.ColumnType[T]; … }
  type ColumnType[T] <: TypedType[T]
```

`self.ColumnType` is a higher-kinded member behind the self alias, so it is now
a path member and the alias records the outer declaration instead of resolving
its own right-hand side back to itself. `MODE=a tests/slick_run.sh` -- the
direction that makes real scalac read our pickles -- is 0/12 both before and
after this change, for reasons this slice does not touch (the pickle loses the
`implicit` flag on evidence parameters, and carries a stub `inline` symbol that
`scala.reflect` refuses outright); so it cannot discriminate, and nothing else
in the battery moved.

### The head after this slice

252 errors in 75 files: **128 `type mismatch`** (95 distinct found/required
pairs), **62 `no matching overload`**, 22 `no implicit`. The prefix-read type
member family is down from 35 error lines to **9**, and those 9 are no longer a
prefix defect -- both sides now say `P.F`.

**The next root in this neighbourhood is a type parameter that occurs only
under an *applied abstract type constructor*, left unsolved.** `Parallel.scala`
line 247 is the clearest instance:

```scala
def parFlatTraverse[T[_]: Traverse: FlatMap, M[_], A, B](ta: T[A])(f: A => M[T[B]])(implicit
  P: Parallel[M]): M[T[B]] = {
  val gtb: P.F[T[B]] = Traverse[T].flatTraverse(ta)(a => P.parallel(f(a)))(P.applicative, FlatMap[T])
```

`flatTraverse[G[_], A, B]` has `B` only inside `G[T[B]]`. We solve `G := P.F`
and leave `B` open, so the call comes out `P.F[T[_]]` against a declared
`P.F[T[B]]`. **24 of the 252 error lines** name such a `_` or the `Nothing` the
same failure collapses to (`Applicative[[γ]Nested[P.F, _, γ]]` against
`Applicative[[γ]Nested[P.F, [β]Either[E, β], γ]]`, `Kleisli[P.F, Nothing, γ]`
against `Kleisli[P.F, A, γ]`, `_[_]` as a required type in `IndexedStateT`'s
six). That is inference, not prefixes, and it is the largest remaining family
with one mechanism behind it.

## The self alias the prefix names (`agent/hkselfalias`)

**A repair slice.** `agent/hkpath` turned `--test tmember` red on `main`, and
neither it nor `agent/projection` ran that suite. `tmember1.scala` models
slick's profile cake:

```scala
trait TypesComponent { self: Profile => type ColumnType[T] <: TypedType[T] }
trait Profile extends TypesComponent { self: Profile =>
  trait API { type ColumnType[T] = self.ColumnType[T] }
}
trait JdbcProfile extends Profile { type ColumnType[T] = JdbcType[T] }
object Main extends JdbcProfile { object api extends API; … }
```

and `api.ColumnType[Int]` at `Main` reported

```
error: type mismatch; found: JdbcType[Int]  required: self.ColumnType[Int]
```

Nothing new was needed to reduce it before: `API`'s alias recorded the bare
declaration, and `expand_type_members`' ordinary name walk -- `from` and then
its lexically enclosing classes -- found `Main`'s own `type ColumnType[T] =
JdbcType[T]`. Lifting the arity restriction from `can_be_path_member` made
`self.ColumnType` a *path member*, and `expand_type_members` refuses a path
member the name walk on purpose (`agent/projection`: re-resolving `q.T` by
name in the class being expanded is what dropping the prefix looked like). So
the alias kept the outer declaration and never met `Main`'s.

### The rule that decides it

A self alias is a term bound in its own class's template scope, so `self.T`
written in `Profile` is `Profile.this.T`. Reading it needs the instance, and
**the instance is what the written prefix names** -- never the class doing the
reading. `SymbolTable::self_alias_member_at` walks `from` and its enclosing
classes for the first one that inherits the self alias's owner, and takes that
class's member of the same name. Two guards keep it from being a widening:

* **a class that leaves `T` deferred answers "no"**, and the path member
  stands. `p.T` and `q.T` on two abstract prefixes must still be two types.
* **a class whose own `T` is an alias naming this very path member answers
  "no"** as well. That is cats' `Representable#compose` -- an anonymous
  `Representable` subclass whose `type Rp = (self.Rp, other.Rp)` names the
  *outer* `Representable` -- and resolving it by name in the subclass is the
  right-hand side folding back onto itself, which is the reason a path member
  is refused the name walk in the first place.

**`from` is the prefix's class, and the distinction is load-bearing.** The
first version of this fix ran the reduction from `this_class`, at the
`expand_type_members(this_class, …)` that `apply_types` already performs on a
written applied type. It fixed `tmember1` and **accepted** this, which real
scalac rejects:

```scala
object Jdbc extends JdbcProfile {           // type ColumnType[T] = JdbcType[T]
  val stolen: Mem.api.ColumnType[Int] = new JdbcType[Int]("INTEGER")
}
```

-- `Mem.api.ColumnType[Int]` is `MemType[Int]` wherever it is written. So the
reduction moved to `Typer::with_prefix_if_type_member`, which already had the
qualifier in hand for its own side table, and `apply_types` now calls
`expand_written_type`: the same walk with the self-alias step suppressed,
because what the source wrote may carry a prefix of its own and the class
being typed is not it. Driving it from the prefix also makes it work from
*outside* any profile (`Jdbc.api.ColumnType[Int]` in an unrelated object),
which the `this_class` version could not do at all.

### The two slick pickles

`agent/hkpath` reported two of slick's 1490 class files not byte-identical --
`RelationalProfile` and `RelationalProfile$RelationalAPI`, `javap -p -c`
identical, the pickle 199 bytes larger -- and named this alias as the cause.
It is, and the growth was a *second* defect, not a consequence of the first.
Reading the pickles back (`scala_signature_bytes` + `read_pickle`) says what
each compiler wrote for `type ColumnType[T] = self.ColumnType[T]`:

| | entry for the right-hand side |
| --- | --- |
| `ab18fc50` (pre-`hkpath`) | `TypeRef(ThisType(RelationalAPI), <the alias itself>)` |
| `9a00edce` (`main`) | a fresh **root-owned** `TypeSym` `ColumnType <: TypedType[T]` |
| here | `TypeRef(ThisType(RelationalProfile), EXTref ColumnType @ RelationalTypesComponent)` |

**Byte-identity with `ab18fc50` is not the right target**: that pickle is a
*cyclic* alias, `RelationalAPI.this.ColumnType = RelationalAPI.this.ColumnType`,
which is the right-hand side folding onto itself in the shape the typer used
to have. `main`'s is wrong the other way: `pickle_type` does replace a path
member by its declaration, but `pickle_type_member` then *mints* a symbol, and
its owner falls back to the root when the declaring class is not in this
pickle -- `ColumnType` is declared in `RelationalTypesComponent`, a different
top-level trait. nsc reads that as `<root>.ColumnType` and reports it missing
from the classpath. `Pickler::pickle_projected_member` writes an external
reference to the real declaration instead, under the `ThisType` the self alias
stands for, which is nsc's own `RelationalProfile.this.ColumnType`.

The class files are 10127 / 9125 bytes against `ab18fc50`'s 10117 / 9113 and
`main`'s 10314 / 9312, `javap -p -c` is identical to `ab18fc50`'s for both,
and the other **1488 files are byte-identical to `main`'s and to
`ab18fc50`'s**. The same root-owned fallback is reachable for an abstract
projection and for a path through a parameter; both predate `agent/hkpath`,
both are written that way on `main` and on `ab18fc50` alike, and widening the
fix to them moves ten more of slick's class files (`BasicProfile`,
`CompilerState`, `JdbcProfile`, `Parameters`, `ShapedValue`, `TableQuery`,
`MemoryProfile`). That belongs with `agent/absproj`, not with a repair slice;
`pickle_projected_member` is deliberately gated on the self-alias case.

### Still not reduced

**An un-applied first-order `p.T`.** `type ColumnType = self.ColumnType` with
no type parameters, read as `api.ColumnType`, is still the abstract member: a
written type with no arguments never reaches `apply_types`, so it never
reaches `with_prefix_if_type_member` either. This is **older than
`agent/hkpath`** -- the fixture has no parameterized member anywhere, so the
arity clause never applied to it -- and it fails on `ab18fc50` the same way.
The first-order path this repairs is the *applied* one.

### Fixtures

`tests/fixtures/hkself_member.scala` runs and prints what real scalac 2.13.16
prints (`expected/hkself_member.txt`), and the e2e test asserts scalac's own
run against that file as well. It carries two profiles that settle
`ColumnType` differently, a profile that leaves it deferred (where the member
must *stay* abstract and still match the declaration `describe` is written
against), a read from outside either profile, and the `Representable#compose`
shape. On `9a00edce` it does not compile at all: seven errors, every one of
them `self.ColumnType` against the concrete type.

`tests/fixtures/hkself_member_bad.scala` is the negative half, and it is the
one that says the prefix still means something. Real scalac 2.13.16 rejects
exactly two of its four reads, at lines 29 and 41. On `9a00edce` **all four**
fail -- the two legal ones included -- so the count, not just the lines, is
what the e2e test pins.

### What it cost elsewhere

cats **251 / 75** (unchanged: the fix must not give back `agent/hkpath`'s 29),
gitbucket 496 / 96, the scala library 1550 / 168, slick `errors=0
files_with_errors=0 classes=1490`, `MODE=b tests/slick_run.sh` `progs=12 ok=12
diff=0 fail=0 attempts=36/36`, `tests/slick_subset.sh` `verified=1490 failed=0
lint_problems=0`.

## The undecided position read as a decision (`agent/catsinfer`)

**251 -> 231 errors, 75 files unchanged.** 20 error *locations* disappeared and
none appeared. `agent/hkpath` named the head as "a type parameter that occurs
only under an applied abstract type constructor, left unsolved" and counted 24
error lines that print a `_` or the `Nothing` it collapses to. **Seven of those
were one root and two more were its mirror image; a third fixed line is a
cascade of the second; and ten of the twenty locations this slice moved were
not in that family at all -- they are a third root nobody had named.** The 24
itself is now 18, and those 18 are three further roots. The honest split is
below.

### The root, which is not in `unify_one`

`unify_one_precise` already descends into the arguments of an applied type
whose constructor is a variable (`Type::Applied` against `Type::Applied` and
against `Type::Class`), and `G[T[B]]` against `P.F[T[Int]]` really does solve
both. The wildcard was never a failure to unify. It was **read out of the
expected type**.

An undetermined variable in an argument's expected type is opened to
`Type::Wildcard` -- `check_apply`'s `relaxed` for a function-typed parameter
whose result mentions one, `open_to_bounds` for a higher-kinded one. That
wildcard means "the argument decides this". `expected_solution` refuses a
*bare* `Wildcard`, and read a solution out of every **nested** one:

```scala
val gtb: P.F[T[B]] = Traverse[T].flatTraverse(ta)(a => P.parallel(f(a)))(P.applicative, FlatMap[T])
```

The literal is typed at `A => _[T[_]]`. Inside it, `P.parallel`'s own
`apply[X](fa: M[X]): P.F[X]` met that expected type, and because an argument of
an application is invariant, `X := T[_]` **outranked** the `T[B]` its own
argument gave. The call came out `P.F[T[_]]`.

Three narrow rules, each measured on its own:

* **A wildcard-bearing expected solution never overrides the arguments'**
  (`add_expected_constraints_in`'s `Some(slot)` arm). Worth the four
  `Parallel.scala` lines.
* **Nor is it pushed for a parameter an argument still to be typed states**
  (the filter after `add_expected_constraints` in `check_apply`). The pass runs
  before the arguments are typed; nsc runs it after. Only inside a relaxed
  expected type -- see below.
* **Nor is it written into a parameter type for a bare lambda's benefit** (the
  `weak` block). `F.map(f(a0).value) { case … }` in `EitherT`/`IorT`/`OptionT`'s
  `tailRecM` took `B := Either[L, _]` off the enclosing `F[Either[L, _]]`,
  which also hid `B` from `open_tparams_of`, so the match that decides it was
  never consulted. Worth those three `(F[_])` lines -- the family the
  `pt_is_undecided` comment in `check.rs` named and `agent/monadtrans` left
  three of. Only inside a relaxed expected type, same as the last.

### A source `_` is not the same wildcard, and only provenance says so

**Refusing every nested wildcard outright costs slick six errors**, and the
recovery from that cost two more measurements before the rules were right.

`options: Set[TableOption[?]] = Set()` is a *source existential*: `Set()` pins
nothing, and `A := TableOption[_]` is the best answer there is. The first
attempt kept it by phrasing the rules as "only where something else in the call
has an opinion" -- which held slick at `errors=0` and cats at 231, and then
**lost `pos/t12899` on the corpus**:

```scala
val c1: Cache[(Seq[String], Class[_]), String] = build { case (sq, cs) => mk(sq, cs) }
```

`K := (Seq[String], Class[_])` carries a wildcard, nothing else in the call has
an opinion about `K`, and the `{ case … }` that would have to be given its
pattern types is exactly the "argument still to be typed" the rule was written
around. It is structurally identical to the cats cases and semantically their
opposite.

`Type::Wildcard` is both nsc's `WildcardType` and (with no bounds) its
`ExistentialType`, so **the two cannot be told apart by shape at all** -- nsc
never has this problem because they are different types there. What separates
them is where the wildcard came from, so `Typer::relaxed_pt_depth` records it:
non-zero exactly while an argument is being typed against an expected type this
compiler relaxed. The two "do not push" rules apply only inside that; the "do
not override a precise answer" rule needs no flag, because a wildcard-bearing
type is less precise than the argument's answer whoever wrote it.

With the flag, slick is `errors=0 classes=1490`, `pos/t12899` passes again, and
cats is the same 231 with the same twenty locations gone.

### The mirror image: an expected type that does say something

`Applicative[[γ]Kleisli[P.F, Nothing, γ]]` against
`Applicative[[γ]Kleisli[P.F, A, γ]]` is the opposite mistake.

```scala
def applicative: Applicative[Kleisli[P.F, A, *]] = catsDataApplicativeForKleisli(P.applicative)
```

`catsDataApplicativeForKleisli[F[_], A]` states `A` nowhere but inside the type
lambda in its result, and a lambda is an anonymous alias symbol whose body
lives beside it in the symbol table (`refinement_type_member`, and
kind-projector's `*` desugars to exactly that). Every arm of `collect_expected`
saw an opaque `TypeMember`, `dealias` will not unfold a higher-kinded alias,
and `A` was minimised to `Nothing`. `eta_expand_pair` -- the step `is_sub_type`
already takes to compare two constructors -- applies both sides to one set of
parameters so the bodies can be walked. Worth `EitherT.scala:1053` (the
`Nested[P.F, _, γ]` line, plus the `ambiguous implicit` its unsolved `A` was
producing at the same line) and `Kleisli.scala:464`.

### Ten more that were a different root entirely

Re-clustering after the two rules above put a family of twelve at the head that
nobody had connected to anything:

```
found: (A) => B          required: (E) => Any
found: (A, A) => A       required: Function2[Any, A, Any]
```

It reduces to five lines with no cats in it:

```scala
class P1[+E, +A] { def map[B](f: A => B): P1[E, B] = new P1[E, B] }
def go[E, A, B](fa: P1[E, A], f: A => B): P1[E, B] = fa.map(f)
```

The lambda parameter of `map`/`flatMap`/`foreach`/`withFilter`/`pipe`/`tap` is
guessed from the receiver's **first** type argument (`elem_type`) when the
signature "has not settled it", and `settled` counted any type parameter at all
as unsettled. Read through `fa: P1[E, A]`, the declaration states its parameter
as the *caller's* own `A` -- in scope, rigid, and already the answer -- and the
guess replaced it with the `E`. A rigid parameter is settled too; the guess is
for one still written in the *declaring* class's parameter, which is not in
scope at the call site. That is `Validated` (4), `Ior` (3), `IorT` (2) and
`NonEmptyMapImpl` (1).

The laxity was also unsound in the other direction: `fa.map((e: E) => …)` on a
`Vd[E, A]` **compiled** before this change, and scalac rejects it.
`tests/fixtures/wci_elem_bad.scala:27` pins it, at scalac's own line.

### Correctness

`tests/fixtures/wci_open.scala` runs all three shapes of the first two rules and
`wci_elem.scala` runs the third; both print, and every line calls a method only
the *right* instantiation has (`mkString` needs `B = String`, `v + 1` needs
`B = Int`, `s.toUpperCase` needs the element and not the error type). The
expected files are what real scalac 2.13.16 prints for the same sources, and
the e2e tests assert scalac's own run against them as well. On the pre-fix
binary `wci_open.scala` fails with exactly one error per shape and
`wci_elem.scala` with two.

`wci_open_bad.scala` and `wci_elem_bad.scala` are the halves that say the
solutions bind: a declared type the call does not produce, a type lambda whose
`A` has no `Show`, an explicit `A` that disagrees with the declared one, and
the two lambda parameters above. Real scalac rejects all five at lines 33, 50,
52, 25 and 27, and so do we, with the same types.

### The cost, measured

Measured twice: on the branch point `9a00edce`, and again after merging `main`
at `66732045` (`agent/linterm` and `agent/gbhead`). Against the merged `main`
alone, gitbucket is 398/83, the scala library 1552/168 and slick `errors=0
classes=1490` -- **all unchanged by this slice**, with cats 251 -> 231 and the
same twenty locations gone. (gitbucket's 496 -> 398 and the library's
1550 -> 1552 are `agent/gbhead`'s, measured on `main` with this branch's three
files checked out to `main`'s versions and back.)

**All 1490 slick class files are byte-identical** to the pre-fix build, so the
one change that reaches codegen changes nothing there. `MODE=b
tests/slick_run.sh` is 12/12 36/36, `tests/slick_subset.sh` is 184 files / 1490
classes / `verified=1490 failed=0` / `lint_problems=0`, and
`tests/verify_all.sh` reports `verify_failures=0` (the two `INCOMPLETE`
`slick.jdbc` singletons want a JDBC driver and are identical before and after).

On the scala/scala corpus (`CORPUS_SIZE=full`), `losses=0` against
`tests/baselines/corpus-d056a7f7.tsv`, with **`pos/t6895` newly passing** --
`barFoo(null) : Foo[({type L[X] = Bar[StringOr, X]})#L]`, the type-lambda
expected type of the second rule, found by the corpus rather than by cats.

### The head after this slice

231 errors in 75 files: **112 `type mismatch`** (85 distinct found/required
pairs), **59 `no matching overload`**, 22 `no implicit`. The wildcard/`Nothing`
family the previous slice counted at 24 is down to **18 lines**, and they are
now three separate roots, none of them the one this slice fixed:

* **Higher-order unification: inventing a type lambda** (9 lines, the largest).
  `traverse(fa)(a => State(s => f(s, a)))` has to solve `G[B]` against
  `IndexedStateT[Eval, S, S, B]`, i.e. `G := [x]IndexedStateT[Eval, S, S, x]`.
  Nothing here constructs a lambda during inference -- `refinement_type_member`
  only allocates one for a *written* refinement -- so `G` stays open and prints
  as the required `_[_]`, taking `Traverse.scala` 143/161/177 and
  `TraverseFilter.scala` 146 with a `value run is not a member of _[F[_]]`
  cascade each. This is nsc's `solvedTypes` with an `HKTypeVar`, and it is a
  real piece of machinery, not a guard.
* **A parameter fixed from the first of two arguments without lubbing the
  second** (4 lines). `EitherT(cata(Left(left), Right.apply))` in `OptionT`
  fixes `cata[B]`'s `B` to `Left[L, Nothing]` and then checks the eta-expanded
  `Right.apply` against it; nsc lubs the two to `Either[L, A]`.
  `unify_tparam_all` does lub two contributions -- a *by-name* first argument
  and an eta-expanded second do not both reach it.
* Singletons: `EitherK`'s `Nothing`, `Kleisli.scala:79`'s `(_) => F[C]`,
  `Traverse.scala:209`'s `Some[F[_]]` (a cascade of the first bullet, same
  file), `ApplicativeError[F, _ >: E]`.

Outside that family the next largest single mechanisms are `Ordering[AA]`
against `Ordering[A]` (6, `agent/catseta`'s leftover), `Map[K, B]` against
`SortedMap[K, B]` (4) with `Set[A]` against `SortedSet[A]` (2), and
`NonEmptyList[AnyRef]` against `NonEmptyList[C]` (4).

## Inventing the type lambda that was already there (`agent/hkunify`)

**231 -> 215 errors, 75 -> 73 files**, measured on `8554717c` and again after
merging `main` at `7aa47c29` (`agent/linorder2`), the same 16 error locations
gone each time and none appeared. `agent/catsinfer` left this family at the
head with a warning: "nothing here constructs a lambda during inference -- this
is nsc's `solvedTypes` with an `HKTypeVar`, real machinery, not a guard".
The machinery turned out to be smaller than the warning, because the
representation already had the shape nsc's rule produces.

### What nsc actually restricts it to

The general problem -- which of `IndexedStateT[Eval, S, S, B]`'s four
positions becomes the parameter of the lambda that solves `G[B]` -- is
undecidable, and nsc does not attempt it. `TypeVar.unifyFull` in
`scala/reflect/internal/Types.scala` (the rule from scala/bug#2712, on by
default since 2.13) reads the constructor as *curried*: with the variable
applied to `k` arguments and the type to `n >= k`, the leftmost `n - k`
arguments are captured as constants and only the rightmost `k` are matched
against the variable's; fewer than `k` is no solution at all, and the kinds of
the abstracted parameters must be those of the variable's (`unifiableKinds`).
That is the whole rule. `Either[String, Int]` against `F[A]` is
`F := Either[String, *]`, `A := Int` -- and never `[x]Either[x, Int]`, however
well that would fit whatever else the call says. Real scalac 2.13.16 confirms
the shape on every fixture line (`-Xprint:typer` shows
`go[[A]St[S,A], A, B]`, `two[[Y, Z]Tri[Int,Y,Z], String, Boolean]`,
`pick[[+R]Int => R, String]`); the cloned parameters keep the class's own
variance.

For a *lower* bound (an argument against a parameter, the direction these
cats lines need) nsc also tries the argument's parents and base types when the
type itself does not unify; for an upper bound (the expected type) only the
type and its alias chain. And **in a covariant position of the expected type
nsc does not partially unify at all**: `def mk[G[_], A](a: A): G[A]` checked
against `Either[String, Int]` is `mk[Nothing, Int]` -- `Nothing` is
kind-polymorphic and minimisation wins. Only an invariant or contravariant
position (`Inv[G[A]]`, `G[A] => Int`) captures. The fixture pins both, and
this compiler still leaves such a covariant `G` unsolved rather than
minimising it to `Nothing`; that is a separate gap and not touched here.

### The representation already curries

A type lambda in this compiler is an anonymous alias symbol (`agent/hkpath`),
and inventing one per unification would have needed the memoisation that
slice paid for. It is not needed: a `Type::Class { sym, args }` with fewer
arguments than the class has parameters **is** the curried constructor --
`kind_arity` subtracts what is applied, `apply_type_ctor` appends the rest, and
`is_sub_type`, `eta_expand_pair` and the implicit search's `Unify` all
already read it as a constructor of the remaining arity. So
`G := Class { IndexedStateT, [Eval, S, S] }` is nsc's
`PolyType([x], IndexedStateT[Eval, S, S, x])`, two spellings of it are
structurally equal, and the implicit `Applicative[G]` is answered by cats'
`Monad[IndexedStateT[F, S, S, *]]` through the eta-expansion that was already
there. The same holds for an alias applied to a prefix
(`Applied { State, [S] }`) and for a lambda that captured enclosing parameters
(`refinement_type_member` hands those out partially applied for exactly this
reason).

What changed:

* **`unify_one_precise`** (`check.rs`): the `Applied` pattern against a
  `Class`, an `Applied`, a `Function` (read as `FunctionN`) or a `Tuple` (read
  as `TupleN`) actual captures the surplus (`partial_unify_applied`). It now
  takes the symbol table, threaded through every caller, because the two
  guards need it: a class with parameters still to come is not a type an
  application can be matched against, and the kinds of the abstracted
  parameters have to be the variable's (`ctor_kinds_unify`). Before, the arm
  solved `G := IndexedStateT` -- the bare four-parameter class -- and zipped
  `B` against `Eval`, which is what every `no matching overload for (F[A])…`
  in this family was. When the class itself does not unify, its parents are
  tried and then theirs (`unify_applied_via_parents`), each seen from the type
  it is reached through -- nsc's `registerBound` for a lower bound. The first
  full corpus run said why that half is not optional: `pos/hkrange`
  (`Range` for `CC[Int]`), `pos/t2693` (`new T[Int] {}` for `T[A]`) and
  `pos/t2712-2` (`CB extends A[Boolean, Long] with B[Boolean, Double]` for
  `M[A]`) had all been *passing* on the old arm's ill-kinded `CC := Range`,
  and the arity guard alone turned them into losses. With the walk they pass
  for nsc's reason, and with nsc's answer: `f(1 to 5)` is an
  `IndexedSeq[Int]` under both compilers (checked by assigning it to a
  `String` and reading the mismatch).
* **`is_sub_type`** (`symbol.rs`): `_[_]`, the shape `check_apply` relaxes
  `G[B]` to while the deciding argument is typed, admitted only an applied
  *abstract* constructor. nsc's `appliedType(WildcardType, args)` is
  `WildcardType`; here the application is kept so its arity stays visible, and
  anything with at least that many type arguments is under it. This is what
  printed `required: _[_]`.
* **`collect_expected`** (`check_infer.rs`): the same capture for an expected
  type with more arguments than the result applies, in the positions nsc
  captures in.
* **`section_param_types` / `undo_eta_param_types`** (`check_overload.rs`,
  `check_apply.rs`): `traverse(fa)(a => State(s => f(s, a)))` has a second
  root. `State.apply[S, A](f: S => (S, A))`'s `S` is undetermined while its
  literal is typed, and the literal's `s` was typed at the bound, `Any`
  (`found: Any  required: S`, the `.run(init).value` cascade with it). nsc's
  `typedFunctionUndoingEtaExpansion` (2.13's, which no longer requires every
  argument to be a parameter) types the body's callee first and reads the
  parameter type off it -- `f: (S, A) => (S, B)` says `s: S`. The existing
  placeholder-section rule already did this for a monomorphic *method*
  callee; a function *value* is a callee too, and a parameter of the enclosing
  method is a fixed type there (one in `undet_tvars` is not). It is consulted
  only for a parameter position that mentions a variable this call has not
  decided, so a written `Any => Int` still types its parameter as `Any`.
* **`fill_undecided`** (`check_infer.rs`): `Traverse.scala:209`, which the
  previous slice counted as a cascade of this family, was not one. cats'
  `mapAccumulate(0L, fa)((i, a) => if (i == idx) (i + 1, b) else (i + 1, a))`
  types its literal at `(Long, _)`, and an `if` under a *tuple* expected type
  had no arm to decide the `_` from its branches the way a `match` under
  `F[_]` does. One arm; `Some[F[_]]` became `Some[F[B]]`.

### The honest split of the nine

The previous slice's "9 lines, one root" were, by location: `Traverse.scala`
143 (four errors), 161 (two), 177 (two), 209 (one), and `TraverseFilter.scala`
145/146 (three). Of those thirteen error lines, **nine were partial
unification** (161, 177, 145/146 and two of 143's four), **three were the
missing-parameter-type root** at 143 (`Any` for `s`, the `(Any, F[B])` result,
and the `ambiguous implicit` that an unsolved `G` produced), and **one (209)
was the tuple `if`**. Partial unification also took **four lines nobody had
connected to it**: `EitherT.scala` 1038/1069/1131, `Nested(fa: F[G[A]])`
given a `P.F[Validated[E, A]]`, whose `G` is `Validated[E, *]`; and
`instances/sortedMap.scala:69`, `mapAccumulateFromStrictFunctor(init, fa, f)`
on a `SortedMap[K, A]`, whose `F` is `SortedMap[K, *]`. Sixteen locations.

### Correctness

`tests/fixtures/hku_partial.scala` runs every shape and prints, for each, the
name of the type an implicit was found for -- so the *solution* is what is
compared, and an instantiation that merely compiles cannot pass:
`pick(e: Either[String, Int])` prints `Int`, `two(new Tri[Int, String,
Boolean])` prints `String,Boolean`, and cats' line 143 is there in its own
spelling with a four-parameter `IxSt[F[_], SA, SB, A]` behind a `St[S, A]`
alias and an `Ap[St[S, *]]` instance to find. `expected/hku_partial.txt` is
what real scalac 2.13.16 prints for the same source, and the e2e test asserts
scalac's own run against it as well. On the pre-fix binary (`8554717c`) the
fixture does not compile: 17 errors, every shape among them.

`tests/fixtures/hku_partial_bad.scala` is the half that says the rule is a
restriction: `both(e, new Inv[String])` for `def both[F[_], A](fa: F[A], a:
Inv[A])` would type-check under `[x]Either[x, Int]` and nsc refuses it
(`A := Any`, and an invariant `Inv[String]` is not that); a declared
`Either[Int, String]` asks for the other abstraction; a two-parameter variable
meets `Option[Int]`; and `Foo[Int, List]` for `G[A]` puts a constructor in the
abstracted position. Real scalac rejects all four at lines 16, 19, 22 and 26,
and so do we, at the same lines. The pre-fix binary rejected them too -- for
the wrong reason at 19 (`no matching overload`, because it could not unify at
all) -- so the negative half is what stops the new arms from over-reaching,
not what shows they exist.

### The cost, measured

On the merged tree (`7aa47c29`): gitbucket 398/83 unchanged, slick `errors=0
classes=1490` with **all 1490 class files byte-identical** to the pre-fix
build (`SLICK_OUT` on both binaries, `diff -r` empty), the scala library
1552 -> **1551** (`sys/process/BasicIO.scala:57`, a `LazyList[_]` the tuple
arm now decides), `MODE=b tests/slick_run.sh` `progs=12 ok=12 diff=0 fail=0
attempts=36/36`. `slick_subset.sh` and `verify_all.sh` were not run: nothing
here reaches codegen, and the byte-identical class files say so more directly.
No new clippy warnings; the 37 in the typer crate are all in untouched code.

On the scala/scala corpus (`CORPUS_SIZE=full`, 5324 units) against
`tests/baselines/corpus-d056a7f7.tsv`: **`losses=0`**, 17 gains. Nine of
them are this slice's, checked by rerunning them on the `8554717c` binary
where they fail: `pos/t2712-1`, `-3`, `-4`, `-7` and `neg/t2712-2` (the
SI-2712 partial-unification tests themselves), `pos/hk-infer`, `pos/t5683`,
`pos/tcpoly_infer_implicit_tuple_wrapper`, and `pos/fun_undo_eta` -- the
corpus's own test for the undo-eta parameter typing. The other eight
(`pos/t10714`, `t10714b`, `t6895`, `t7753`, `t8801`, `run/t102`, `run/t3798`,
`neg/t7507`) already pass on `8554717c`; they are earlier slices' gains the
ledger predates. The candidate is `pos 1086 / neg 670 / run 618`.

### The head after this slice

215 errors in 73 files: **106 `type mismatch`** (79 distinct pairs), **61 `no
matching overload`**, 22 `no implicit`, 7 `ambiguous implicit`. The
wildcard/`Nothing` family that stood at 18 lines is **11**, none of them
partial unification: `OptionT.scala` 496/510/524/538 (4) are the
"parameter fixed from the first of two arguments without lubbing the second"
root the previous slice named (`cata(Left(left), Right.apply)`);
`Validated.scala:1128` and `syntax/either.scala:405` (`Either[Any, Nothing]`
against `Either[Throwable, A]`, a `catchNonFatal` shape) are one more root;
and `EitherK.scala:60`, `Kleisli.scala:79`, `WriterT.scala:182` (two) and
`syntax/option.scala:395` are singletons. The largest single mechanisms
outside it are unchanged: `Ordering[AA]` against `Ordering[A]` (6), `Map[K,
B]` against `SortedMap[K, B]` (4) with `Set[A]` against `SortedSet[A]` (2),
`NonEmptyList[AnyRef]` against `NonEmptyList[C]` (4), `(A, A) => A` against
`Function2[Any, A, Any]` (4). By file: `OptionT.scala` 11, `Kleisli.scala`
10, `instances/try.scala` 7, `Chain.scala` 7.

## The view that was found and could not be applied (`agent/convimpl`)

**gitbucket 398 -> 393 errors, 83 files unchanged**; cats, slick and the scala
library unchanged. This slice is a repair: the previous one
(`agent/hkunify`, above) left `main` red on two tests, and the root is older
than the change that exposed it.

### The symptom

```scala
final class Bag[A](val a: A)

implicit def toFlatMapOps[F[_], A](fa: F[A])(implicit F: FlatMap[F]): FlatMapOps[F, A]

new Bag(1).flatMap(n => new Bag(n))
```

scalac 2.13.16 reports `value flatMap is not a member of Bag[Int]`. We reported
`could not find implicit value of type FlatMap[Bag]`, twice, at two different
spans on the same line.

Partial unification is what made the second error possible, and it is not
wrong: `F := Bag`, `A := Int` is exactly how nsc solves that parameter too.
Before it, `F` could not be solved at all, the conversion was never applicable,
and `not a member` came out *by accident*. What the accident had been standing
in for is a rule we did not have.

### The rule

nsc's `inferView` types the whole application, implicit clauses included
(`Implicits.scala`, `typedImplicit1` -> `typed1` of the candidate tree). A
failure anywhere in it makes the candidate **not applicable**: the error is
raised inside the search's own context and discarded with the candidate, the
search goes on with the remaining views, and if none survives,
`adaptToMemberWithArgs` falls back on the selection's own diagnostic, at the
selection's own position.

So the rule is a rule of the *search*, not a licence to swallow the
diagnostic. Three shapes separate the two readings, and all three were checked
against real scalac 2.13.16 before anything was written:

| program | scalac 2.13.16 |
| --- | --- |
| the view has no witness, and no other view applies | `value flatMap is not a member of Bag[Int]` |
| the view has no witness, another view does apply | compiles, uses the other view |
| `toFlatMapOps(new Bag(1))`, written out by hand | `could not find implicit value for parameter F: FlatMap[Bag]` |

The third is the one a "never report a conversion's missing witness" rule
would have broken, and it is the case where the message is what the user
actually needs.

### What changed

`Typer::search_extension` collects the conversions whose result has the wanted
member and then narrows the set: duplicates reached twice, conversions a
subclass overrides, members that cannot take the call's arguments. A fourth
pass, `drop_witnessless_conversions`, now runs among them and drops any
candidate whose own implicit clauses cannot be filled. It runs **before** the
tie-breakers, so a witnessless view does not take part in an ambiguity either
-- which is what turned two conversions that used to tie into an answer.

The check, `conv_implicits_available`, warms exactly what `fill_conv_implicits`
warms (the wanted type's implicit scope, then the candidates' parents) and asks
at the same depth. The two must agree in both directions: a clause the search
rejects costs a conversion nsc applies, and one it accepts that the fill cannot
satisfy is the duplicate diagnostic -- `type_select` inserting the view and
`rewrite_apply_extension` inserting it again, each reporting the same failure.
With the search deciding it, neither path reaches the fill with a view that
cannot be completed, and the duplicate is gone by construction rather than by
de-duplicating messages.

One narrow exception: a `ClassTag[A]` whose `A` is still the conversion's own
type parameter is left to `fill_conv_implicits`, which reports `type A is an
unresolved spliceable type`. That says more than a member error.

`conv_param_matches` used to carry half of this rule -- it ran the same search
for the widened "any type constructor applied to one argument" path only, and
under `&self`, so it could not load the class file a witness lives in. Partial
unification made `is_sub_type` accept that shape outright, so the guard stopped
being reached; it is now removed rather than repaired, because the property is
one of the whole application and both callers of `conversion_result` settle it
for themselves.

### Verification

`cimpl_view` (the witness is there: the view applies and the program runs),
`cimpl_other_conv` (two views equally specific by argument type, separated only
by their implicit clauses -- the witnessless one is dropped and the other wins
outright), `cimpl_view_bad` and `cimpl_explicit_bad` (the two rejections). Each
is pinned against scalac 2.13.16, the positive two byte for byte on stdout, on
both the real library and the private runtime. On the pre-fix binary
`cimpl_view_bad` reports the wrong error twice and `cimpl_other_conv` does not
compile at all (`value describe is not a member of Bag[Int]`).

gitbucket's five: two `TypedType[Option[Date]]` and one `BaseTypedType[B1]`
`no implicit` become the member errors nsc would report (`value asc/desc is not
a member of Rep[Option[Date]]`), and two `OptionLift[P, Rep[Option[T]]]` with
their two dependent `no matching overload` lines go away entirely. On the
scala/scala corpus (`CORPUS_SIZE=full`, 5324 units) the candidate is
`pos 1086 / neg 670 / run 618` -- identical to `aa707a3f` -- with `losses=0`
against `tests/baselines/corpus-7aa47c29.tsv` and the nine gains that ledger
predates, all of them `agent/hkunify`'s.

## The forwarder standing next to the declaration (`agent/catstail`)

**215 -> 200 errors on `eb4c9c61`, and 211 -> 196 after merging
`agent/basetypeseq`; 73 files unchanged either way.** Fifteen error
locations gone and none appeared; gitbucket 393 -> 391 as well. The brief for this slice named
six families and asked which of them are genuinely separate. The answer is
that the largest thing in cats was **none of them**: fifteen lines nobody had
connected to each other, spread over four files, wearing three different
messages.

### The head was not any of the six named families

Clustering the 215 by *message* put `Ordering[AA]` against `Ordering[A]` (6)
at the top, and clustering by *file* put `OptionT.scala` (11) there. Both are
the wrong unit. Grouping by the member being selected instead gives:

| n | lines | member |
|---:|---|---|
| 15 | `NonEmptyLazyList.scala` 269/275/281 (x2)/298 (x2)/354, `instances/lazyList.scala` 79/158 (x2), `instances/stream.scala` 67/177 (x2), `instances/arraySeq.scala` 188 (x2) | `reduceLeft` / `foldLeft` / `reduceLeftOption` on a collection |

Three messages -- `found: (A, A) => A  required: Function2[Any, A, Any]`,
`value apply is not a member of B`, and the `found: B  required: A` /
`found: Option[B]  required: Option[A]` that follow them -- and one root.

### The root

scalac gives every concrete class a **mixin forwarder** for each default
method it inherits from a trait, and writes a `Signature` attribute for it.
A JVM generic signature cannot write `[B >: A]`, and the JVM has one argument
list, so the forwarder is a *lossy copy* of the declaration:

```text
scala.collection.IterableOnceOps:  def reduceLeft[B >: A](op: (B, A) => B): B
scala.collection.AbstractIterable: public <B> B reduceLeft(Function2<B, A, B>)

scala.collection.IterableOnceOps:  def foldLeft[B](z: B)(op: (B, A) => B): B
scala.collection.AbstractIterable: public <B> B foldLeft(Object, Function2)
```

`classpath::is_erased_scala_forwarder` drops the forwarders that carry **no**
signature at all -- the ones `agent/final2` found behind
`value map is not a member of Any`. These carry one, so `fill_java_members`
installed them, and a receiver below `AbstractIterable` saw the declaration
*and* its copy. Overload resolution then had two alternatives where nsc has
one, and picking the copy left `B` with nothing but `Any` to be:
`toLazyList.reduceLeft(f)` came out `required: Function2[Any, A, Any]`, and
`toLazyList.foldLeft(b)(f)` read `(b)` as the whole call and applied its `B`
result to `(f)` -- `value apply is not a member of B`.

Both copies have to be present for the failure; one alone still types from
the expected type. That is why it is order-dependent and why it looked like
four unrelated families: `AbstractIterable`'s class file is read when an
`import` pulls in a class whose parent chain reaches it (in cats,
`scala.collection.immutable.ArraySeq`), and `IterableOnceOps`' own
declaration is installed when some *other* receiver asks for the same name.

`Check::drop_classfile_forwarders` (`check_select.rs`, in `drop_overridden`'s
chain) drops such a copy when three things hold: it is a member the class
file reader made (`jvm_name` is a descriptor), a *pickled* declaration with
the same erased parameter list is in the same candidate set on a class above
it, and the copy says exactly what that declaration says minus the two losses
above. The last is `faithful_bytecode_copy`: flatten both parameter lists,
line the type parameters up **by name** (scalac writes the forwarder from the
declaration, so the names are the declaration's), read a `FunctionN` and a
function type as the same thing, and require equality.

### What the narrower forms of the rule cost, measured

Two looser versions were measured and rejected, and both are worth recording
because they are the obvious ones to reach for:

* **"the declaration has more clauses, or a type parameter with a lower
  bound"** -- counting the losses instead of comparing the whole signature.
  That drops copies which say *more* than the declaration: `immutable.List`
  renders the `++[B >: A](that): CC[B]` it inherits as
  `++(IterableOnce): List[B]`, `immutable.Map` renders `getOrElse`'s key with
  the erased `Any`, and `SetOps.++(that: IterableOnce[A]): C` is a genuinely
  different overload from `IterableOps.++[B >: A]` (2.13 keeps both; the
  monomorphic one is strictly more specific). cats went to 202 and **slick
  from 0 to 7 errors**: `xs ++ ys` on a `List` down to `Iterable`, two
  `getOrElse` candidates left with nothing to separate them.
* **"an empty `pickled_origin` means the class file made it"** -- it also
  means *source* made it. slick's `class ProductWrapper extends Product` has
  its own `productArity`, which is not a forwarder for the declaration it
  implements; without the descriptor guard **18 slick class files changed**
  (`invokevirtual ProductWrapper.productArity` became
  `invokeinterface Product.productArity`). Correct at run time, and exactly
  the kind of silent codegen change the byte-comparison exists to catch.

### Correctness

`tests/fixtures/cfw_forwarder.scala` runs the three shapes and prints; every
line is checked by its value, so an instantiation that merely compiles cannot
pass (`fold` accumulates a `String` over `Int`s, which needs `B` to be the
caller's own `B` *and* needs the second clause to exist). The expected file is
what real scalac 2.13.16 prints for the same source, and
`crates/cli/tests/cfwd.rs` asserts scalac's own run against it as well. On the
pre-fix binary the fixture does not compile: five errors, every shape among
them.

`tests/fixtures/cfw_forwarder_bad.scala` is the half that says the rule is a
restriction. `bad2` is the one that matters: `xs.foldLeft(0, f)` passes both
of `foldLeft`'s clauses as one argument list, which is precisely what the
flattened forwarder accepted -- **the pre-fix binary compiles it** and real
scalac rejects it (`missing argument list for method foldLeft`). Both lines
are rejected at scalac's own 25 and 29.

### The cost, measured

Measured twice: on `main` at `eb4c9c61` (`agent/gbshape`, `agent/gbmapto`),
and again after merging `main` at `3b9a61ed` (`agent/basetypeseq`), each
time against that same `main` measured on its own. The delta is identical
in both: cats **-15** (215 -> 200 on the first, 211 -> 196 on the second --
`agent/basetypeseq` takes four of the same lines), gitbucket **-2**
(393 -> 391 both times), the scala library **unchanged** (1551 -> 1551, then
1420 -> 1420), and slick `errors=0 files_with_errors=0 classes=1490` with
**all 1490 class files byte-identical** to `main`'s build both times
(`SLICK_OUT` on both binaries, `diff -r` empty). `MODE=b
tests/slick_run.sh` is `progs=12 ok=12 diff=0 fail=0 attempts=36/36`.
`slick_subset.sh` was not run: nothing here reaches codegen, and the
byte-identical class files say so more directly. On the scala/scala corpus
(`CORPUS_SIZE=full`) `pos 1086 / neg 670 / run 618`, with `losses=0` **and
no changes at all** against `tests/baselines/corpus-8d4cdde0.tsv`. No new
clippy warnings (pickle 0, typer 37, all in untouched code).

### The pickle's linearization is not ordered by derivedness (not fixed)

The `SortedMap` family the brief named -- `instances/sortedMap.scala`
73/78/93/237/242/247, `NonEmptyMapImpl.scala` 114/122 -- was diagnosed in
full, and a fix was written, measured and **reverted**. Recorded here so the
next slice starts from the diagnosis rather than the symptom.

`SortedMapOps` overloads `map` / `flatMap` / `collect` with an
`(implicit ordering: Ordering[K2])` clause returning the *sorted* collection,
which is why cats writes `implicit val ordering: Ordering[K] = fa.ordering`
one line above each of them. `PickleSupply::install` keeps one declaration per
erased *explicit* parameter list -- the two are equally specific under nsc's
`isAsSpecific`, which looks through an implicit clause, so only their owners
separate them -- and it keeps the **first** one `SigCache::lookup` offers, on
the reading that the walk is most-derived-first.

The walk is not. `SigCache::lin_of` folds the parents as
`acc = L(Ci) ++ (acc minus L(Ci))`, which gives a shared ancestor the
**later** parent's position; SLS 5.1.2's `+:` replaces the elements of the
*left* operand, so the earlier parent's position wins
(`crates/typer/src/lin.rs` states the rule and gets it right).
`immutable.SortedMap`'s last parent is
`SortedMapFactoryDefaults extends SortedMapOps[…] with MapOps[…]`, so
`collection.MapOps` headed the list and `aSortedMap.map(f)` was supplied as
`MapOps.map[K2, V2](f): Map[K2, V2]`, with `SortedMapOps.map` discarded as
"shadowed by a more derived declaration" by a declaration that is in fact
less derived. The trace is one line of `SCALA_RS_PICKLE_DEBUG=1`:

```text
[pickle] scala.collection.immutable.SortedMap#map: hit from scala.collection.MapOps :: …
[pickle] scala.collection.immutable.SortedMap#map: hit from scala.collection.SortedMapOps ::
         [K2, V2](f: …)(implicit ordering: Ordering[K2])scala.collection.immutable.SortedMap[K2, V2]
[pickle] scala/collection/immutable/SortedMap#map: skipping an overload shadowed by a
         more derived declaration with the same parameters
```

Three things were measured on top of that:

1. **Correcting `lin_of` to SLS's `+:`.** It fixes `map`/`flatMap`/`collect`
   and breaks the *substitution*: a step carries not only a position but the
   substitution its route produces, and a class reached twice is then read at
   whichever route holds the position rather than at its most derived
   instantiation. `immutable.Set` reaches `collection.SetOps` as
   `C = collection.Set[A]` through its second parent and as
   `C = immutable.Set[A]` through its third; scalac types `x | y` on two
   `immutable.Set[A]`s as `immutable.Set[A]` (`-Xprint:typer`), because nsc
   merges the parents' base type sequences *contravariantly*
   (`BaseTypeSeqs.CompoundBaseTypeSeq`) rather than picking one by position.
   With the order corrected and no merge, cats went 215 -> 219 and slick 0 -> 1
   (`errors += RefId(n1)` in `VerifyTypes.scala`).
2. **The order plus a most-derived merge of the substitutions.** cats 217
   (nine lines fixed, eleven appeared), slick back to 0. Of the eleven, six
   were the forwarder root above, one was `apply is not a member of B`, three
   were the `lazyZip` ambiguity and one was `Tuple2[A, B]` against `(A, B)`.
3. **Ordering the *hits* rather than the walk** (`order_by_derivedness`,
   sorting `lookup`'s answers so a declaration precedes every declaration it
   overrides, leaving every substitution alone). This is the one to build on:
   it fixes the whole family in a one-file repro, leaves slick at `errors=0`
   with byte-identical class files, and leaves gitbucket and the scala library
   unchanged. Two things stopped it landing here. It is **inert on cats as
   measured**, because by the time cats reaches `instances/sortedMap.scala`
   the name is already installed and completion never runs -- it needs
   `Check::supply_receiver_override` to walk the receiver's *ancestors* (its
   `declares_other_signature` gate asks only the receiver's own class file,
   and `immutable.SortedMap`'s declares no `map`, no `collect`, no `keySet`).
   And with that walk it costs `crates/cli/tests/ambigmap.rs`'s
   `am_pickledup`, whose two copies of one pickled declaration stop
   collapsing -- `collapse_pickled_copies` dedups by `pickled_origin`, and
   re-ordering the hits gives the two receivers different origins.

The one-file repro, which real scalac 2.13.16 accepts in full:

```scala
import scala.collection.immutable.{SortedMap, SortedSet, BitSet}
object S1 {
  def m1[K, A, B](fa: SortedMap[K, A])(f: A => B): SortedMap[K, B] = {
    implicit val ordering: Ordering[K] = fa.ordering
    fa.map { case (k, a) => (k, f(a)) }          // found: Map[K, B]
  }
  def m5[K, A](fa: SortedMap[K, A]): SortedSet[K] = fa.keySet   // found: Set[K]
  def m7(x: BitSet, y: BitSet): BitSet = x | y                  // found: SortedSet[Int]
}
```

`keySet` and `BitSet`'s `|` are *not* the same root and are not fixed by any
of the three: `SortedMapOps.keySet: SortedSet[K]` is a plain covariant
override with the same erasure, which `supply_receiver_override`'s
`declares_other_signature` refuses by design (installing one renames the call
target), and `BitSet`'s `C` is the base-type merge of (1) above --
`agent/basetypeseq`'s subject on the symbol-table side.

### The head after this slice

200 errors in 73 files on `eb4c9c61` (96 `type mismatch` over 73 distinct
found/required pairs, 61 `no matching overload`, 22 `no implicit`, 7
`ambiguous implicit`), and **196 in 73 files** after merging
`agent/basetypeseq`: **95 `type mismatch`** over **72** distinct pairs, **58
`no matching overload`**, 22 `no implicit`, 7 `ambiguous implicit`. That
slice takes `Chain.scala` 1084/1085, `NonEmptyLazyList.scala:378` and
`NonEmptySeq.scala:294` -- the `found: Seq[A]  required: Seq[A]` shape --
and nothing else here moves. By file: `OptionT.scala` 11, `Kleisli.scala`
10, `instances/try.scala` 7, `syntax/either.scala` 6. The largest single
mechanisms, re-clustered:

* **`SortedMap`/`SortedSet` losing their ordering** (15, and *two* roots, not
  one). The `(implicit Ordering[K2])` overload, diagnosed above with a
  measured route in, is ten: `sortedMap.scala` 73/78/93/237/242/247,
  `NonEmptyMapImpl.scala` 114/122, `sortedSet.scala:110`,
  `NonEmptySet.scala:308`. The other five are the base-type merge, not the
  hit order: `sortedSet.scala:34` and
  `kernel/instances/SortedSetInstances.scala:106` (`x | y`, whose `C` is read
  through `immutable.Set` rather than through `SortedSetOps`),
  `NonEmptyMapImpl.scala` 129 (`transform`) and 148 (`keySet`, a plain
  covariant override with the same erasure), and `NonEmptySet.scala:418`.
* **`Ordering[AA]` against `Ordering[A]`** (6) -- `agent/catseta`'s leftover,
  untouched by four slices now.
* **`Iterator[Iterable[A]]` against `Iterator[<the concrete collection>]`**
  (3, at `NonEmptyLazyList.scala:462`, `NonEmptySeq.scala:364` and
  `NonEmptyVector.scala:357`, each with a `(Iterable[A]) => Any` companion on
  the same line, and `instances/stream.scala:64` a fourth) -- a
  `sliding`/`grouped` result whose element type collapsed to the base.
* **`no matching overload for (Factory[B, C1])C1`** (6): `arraySeq.scala`
  107/109/237, `ChainCompanionCompat.scala:44`, `lazyList.scala:76`,
  `compat/SortedSet.scala:29` -- `to(factory)` / `from(factory)`.
* **`NonEmptyList[AnyRef]` against `NonEmptyList[C]`** (4).
* **`Some[To]` against `Option[List[A]]`** (4): `instances/list.scala:45`,
  `queue.scala:44`, `seq.scala:44`, `vector.scala:43`.
* **`OptionT.scala` 496/510/524/538** (4) -- the "parameter fixed from the
  first of two arguments without lubbing the second" root
  (`EitherT(cata(Left(left), Right.apply))`), unchanged and still unclaimed.
* **`ambiguous overload for lazyZip`** (4): `ZipLazyList.scala:39`,
  `ZipStream.scala:41`, `arraySeq.scala` 204/206.

The `(A, A) => A` against `Function2[Any, A, Any]` family the brief listed at
4 lines is **gone**: it was this slice's root, and it was 15 lines rather than
4 because two of its three messages had been counted as other things.

## `SortedMap`'s implicit-clause overload (agent/sortedmap)

`agent/catstail` left three measured variants and reverted all of them.
`agent/basetypeseq` then landed and changed the thing variant 1 tripped over,
so all three were **re-measured on `2fdfe302`** before anything was chosen.
The numbers had moved:

| on `2fdfe302` | cats | slick | gitbucket | the one-file repro |
|---|---:|---:|---:|---|
| baseline | 196 | 0 / 1490 | 337 | 4 errors |
| 1. SLS order in `lin_of` alone | **198** | **4** | — | 5, and `BitSet`'s `\|` regressed |
| 2. that order + a most-derived merge of the substitutions | 196 | 0 / 1490 | **339** | **1** (`keySet` only) |
| 3. `order_by_derivedness` on the hits | 196 | 0 / 1490 | **339** | **1** (`keySet` only) |

Variant 1 is still wrong: `basetypeseq` fixed the base-type merge on the
*symbol table* side, and `SigCache::lin_of` has a substitution of its own that
nothing else repairs -- with the order corrected and no merge, `fa.map(f)` came
back `found: SortedMap[K, B]  required: SortedMap[K, B]`, the two being
`collection.SortedMap` and `immutable.SortedMap`.

Variants 2 and 3 are indistinguishable on every number measured, and **both
were built out in full and then abandoned**. Written down because the second
half of that work is what says why:

* Both are inert on cats until `Check::supply_receiver_override` walks the
  receiver's *ancestors* (below). With that, both reach **cats 188**.
* Both then cost `crates/cli/tests/ambigmap.rs::am_pickledup`. The cause is
  not the order: it is that `install` keeps **one link of an override chain**
  and which link depends on where the receiver enters it, so
  `collection.IndexedSeq` reaches `IndexedSeqOps.map` where `immutable.Seq`
  reaches the `IterableOps.map` it overrides, and on `immutable.IndexedSeq`
  the two copies no longer collapse. Recording the chain on the symbol
  (`Symbol::pickled_shadows`, plus a `drop_pickled_shadowed` rule) fixes that
  and is a sound piece of work -- comparing the origins by *symbol-table*
  ancestry is not, because `scala.collection.IterableOps` has no class symbol
  at all in a program that never names it.
* What killed them is the rest of the fallout, which the numbers hid.
  Reordering makes the **more derived** declaration win everywhere, and
  `MapOps.map[K2, V2](f: ((K, V)) => (K2, V2))` is more derived than
  `IterableOps.map[B](f: A => B)` **without being an override of it** -- it is
  an overload with a narrower parameter, and nsc keeps both and picks by
  expected type. `cargo test --workspace` came back `2396 passed, 3 failed`
  and the corpus `losses=2`: `crates/cli/tests/buildfrom.rs`'s
  `bf_coll_runs_against_the_jar` **threw at run time**
  (`java.lang.ClassCastException ... at MapBuilderImpl.addOne`), because
  `Map("a" -> 1).map { case (_, v) => v }` compiled and then built a `Map` out
  of `Int`s. The gitbucket +2 was the same root
  (`IssuesService.scala` 1141/1152/1156).
* Two repairs for *that* were measured and rejected in turn: preferring the
  least derived declaration at equal arity (cats 193 -- five new lines of
  `value applyOrElse is not a member of (E) => F[A]`), and the same rule
  extended to the `seen_shapes` key (**gitbucket 1150**).

### What landed

No reordering at all. The linearization the pickle walks is untouched, and so
is every collapse it drives, with exactly one exception:

> A declaration that adds an **implicit clause** to one it inherits supersedes
> it.

`SortedMapOps.map[K2, V2](f)(implicit ordering: Ordering[K2])` has the same
*explicit* parameters as the `MapOps.map[K2, V2](f)` it inherits, so the two
share `install`'s key; nsc's `isAsSpecific` looks through an implicit clause,
so specificity does not separate them either, and the walk offered `MapOps`
first. The extra clause is the `Ordering` witness -- it is the whole reason the
declaration exists and the only way the result can be the receiver's own sorted
collection -- so where it is present, and the class declaring it derives from
the class declaring the other (asked of the *pickle*: the symbol table has no
`scala.collection.MapOps` in a program that never names it), the longer
declaration replaces the shorter one. Two declarations with the **same** number
of parameters are left exactly as they were, which is what keeps
`IterableOps.map[B]` in front of `MapOps.map[K2, V2]` for a plain `Map`.

The superseded member is detached from the class only once the replacement is
known to install -- an alternative with no usable descriptor must not take the
place of the one already in -- and is dropped from what
`supply_member_from_pickle` hands back, or it would reach overload resolution
as an alternative nothing can call.

### `supply_receiver_override` asks the ancestors

That alone is inert on cats, for the reason `agent/catstail` gave: by the time
cats reaches `instances/sortedMap.scala` the name is already installed and
completion never runs. `Check::supply_receiver_override`'s
`declares_other_signature` gate asked the receiver's *own* class file, and
`scala/collection/immutable/SortedMap.class` declares no `map`, no `collect`
and no `keySet` -- they belong to `collection.SortedMapOps`. So the gate
answered "no" for exactly the family it exists to admit. It now walks the
receiver's linearization and stops at the first class that already owns a
candidate: past that point a declaration is not *newer* than the answer in
hand, it is the answer in hand or something it overrides.

### The cost, measured

cats **196 -> 188** (`instances/sortedMap.scala` 73/78/93/237/242/247 and
`NonEmptyMapImpl.scala` 114/122; **no new line anywhere**), gitbucket
**337 -> 337**, the scala library **1420 -> 1420**, slick `errors=0
files_with_errors=0 classes=1490`. Of slick's 1490 class files **1489 are
byte-identical** to the pre-fix build (`SLICK_OUT` on both binaries,
`diff -r`). The one that differs is
`slick/basic/ConcurrencyControl$ConnectionArbiter`, where `aTreeMap - k` goes
out as `invokevirtual TreeMap.$minus(Object)MapOps` instead of
`invokeinterface immutable.MapOps.$minus` -- the ancestors walk completes
`$minus` on `TreeMap` itself, and `immutable.AbstractMap`, which `TreeMap`
extends, declares `public final MapOps $minus(Object)`, so the resolution is a
class method where it was an interface default. Everything else in that file is
constant-pool renumbering behind it.

### The second root, and a third

Nothing here touches the other lines `agent/catstail` separated out, and
`agent/basetypeseq` took `BitSet`'s `|` in the one-file repro but not in cats.
What is left of the family is seven lines, all on the `SortedSet` side:

* `x | y` on a `SortedSet` (`instances/sortedSet.scala:34`,
  `kernel/instances/SortedSetInstances.scala:106`, `BitSetInstances.scala:51`)
  -- `SetOps.|` is read through `immutable.Set`, whose `C` is `Set[A]`, rather
  than through `SortedSetOps`. That is the base type sequence, not the
  collapse.
* `keySet` (`NonEmptyMapImpl.scala:148`) and `transform` (`:129`) -- a plain
  covariant override with the **same erasure and the same arity**, so neither
  the rule above nor `supply_receiver_override` will take it:
  `declares_other_signature` refuses a same-erasure override *by design*,
  because installing one renames the call target and that turned
  `aSet.toSeq.length` into a `VerifyError`. What it needs is for the override
  to be installed *without* renaming the call -- the caller's type from the
  pickle, the descriptor from the class file, which is the split `install`
  already performs for `decl_site_want`.
* `NonEmptySet.scala` 308/418 and `instances/sortedSet.scala:110`, the same two
  roots seen through cats' `Newtype`.

And a **third root**, found by running the fixture rather than compiling it,
present on the pre-fix binary as well: `immutable.SortedSet` is hand-written in
the prelude (`prelude_ordering2::add_sorted_set`) with `contains` and `foreach`
and nothing else, so `aSortedSet.map(f)` binds the prelude's `Set.map` and
`Check::rebuild_from_receiver` narrows the result to `SortedSet[B]` -- with no
`Ordering[B]` witness anywhere. `rebuild_widened` refuses exactly this
(`needs_ordering_to_rebuild`), but the `method_name == "map"` arm of
`Typer::type_apply_in` (`check_apply.rs`) reaches `rebuild_from_receiver`
directly and never asks. The program compiles, the call goes out as
`IterableOps.map(Function1)`, the value is a `Set$Set3`, and the narrowing is a
`ClassCastException` at the first use. Gating that arm would turn a silent
wrong answer into a false rejection, so the fix is the same as `keySet`'s:
supply `SortedSetOps.map` from the pickle. `tests/fixtures/sm_ordering.scala`
says in a comment why it does not exercise it.

## The `agent/basetypeargs` slice: a nested class read from a pickle

The brief for this slice named `SymbolTable::base_type_args`' first-path
behaviour as the root of a `sliding` / `grouped` cluster. **That premise is
stale**: `agent/basetypemeet` closed the first-path defect two gates earlier,
and this slice measured it — `base_type_args` is correct here, and neither of
the two roots it found is in the symbol table's base-type walk.

Measured on the branch merged with `main` at `d0c89fc1`, against a binary built
from that same `main`:

| | before | after |
|---|---:|---:|
| cats (339, 1 skipped) | 177 / 69 files | **176 / 68** |
| scala library (538) | 622 / 131 | **614 / 131** |
| gitbucket (353, 1 skipped) | 265 / 77 | 265 / 77 |
| slick (184) | `errors=0 classes=1490` | `errors=0 classes=1490` |

Nine errors go and **no error appears anywhere**, compared line by line rather
than by total. The same nine, and the same delta, at the branch point
`5800b6ea` (cats 182 -> 181, library 672 -> 664), so the waves do not overlap.

### The one shape that breaks every guess at once

`Iterator[A].sliding(n)` and `.grouped(n)` return `Iterator.GroupedIterator[B]`,
an inner class of `trait Iterator` declared
`extends AbstractIterator[immutable.Seq[B]]`. Its element is `Seq[B]` and its
`CC` is `Iterator` — so it is neither "the receiver's first type argument" nor
"the receiver's own class", and this compiler was assuming both. Three separate
defects met on it:

1. **`PickleSupply::ensure_class` entered a nested class as a package-level
   one.** It split `scala/collection/Iterator$GroupedIterator` at the last `/`
   and nothing else, so the symbol was called `Iterator$GroupedIterator` and
   owned by the package `scala.collection` — a **second symbol** for the class
   `install_java_class_in` enters correctly as `GroupedIterator` inside
   `Iterator`. This is confined to `scala.collection` for the reason in (2):
   lifting it to every nested library class costs
   `engine.rs::rd_reify_shape_expands_and_runs`, where a second `Exprs.Expr`
   under the owner the JVM name implies makes `c.universe.Expr.apply[Int](…)`
   bind the one with no members. Which one a program got depended on which path reached the class
   first, and the printed receiver said so: `Iterator$GroupedIterator[A]` where
   scalac prints `it.GroupedIterator[A]`.

   The order dependence is directly observable on the branch point.
   `def x[A](it: Iterator[A]): Iterator[Seq[A]] = it.sliding(2)` is rejected;
   put `def warm[A](it: Iterator[A]): Seq[A] = it.sliding(2).next()` above it
   and the same expression compiles, because the member lookup drags the class
   file in first. `crates/cli/tests/btargs.rs` pins both orders.
2. **The stub had no parents, and one hop is not a hierarchy.**
   `stub_superclass_from_classfile` declines a nested class that has type
   parameters, on the correct ground that a class file cannot say what
   arguments its superclass is applied at — but its *pickle* can, and
   `ensure_class` is the one place that has already opened it. Attaching only
   `AbstractIterator[Seq[B]]` was still not enough: `AbstractIterator`'s own
   stub was standing at `AnyRef`, so `Iterator` was not a base class either.
   The attachment now walks the chain it creates.

**Both halves are confined to one family: a class nested in `scala.collection`
whose pickled parent names reach `IterableOnce`.** That is decided in
`PickleSupply::pickle_reaches`, from names alone, before any symbol exists --
because what it decides is *how* to build the symbol. Everything about the
restriction is measured, and three wider versions were built and thrown away:

* **every nested library class** costs nine workspace tests and two slick
  errors. `scala.reflect`'s API is not read from its pickle here:
  `prelude_reflect` and `prelude_reflectruntime` build it by hand and
  `reify*.rs` and `macros.rs` reason about the symbols they build, so a second
  `Exprs.Expr` under the owner the JVM name implies makes
  `c.universe.Expr.apply[Int](…)` bind the one with no members, and giving those
  classes their pickled parents stops `ShapedValue.scala`'s quasiquote
  resolving `SyntacticAppliedExtractor`. `tests/verify_merge.sh` returned
  `VERDICT=FAIL` on that version, with `losses=3` on the corpus as well.
* **every nested `scala.collection` class** costs
  `fvg.rs::map_with_filter_overloads_match_scalac`. `MapOps.WithFilter` is
  nested there and is *not* an `IterableOnce`; with its pickled parents,
  `IterableOps.WithFilter`'s `map` and `flatMap` stand in front of the ones
  `Check::map_with_filter_result` is written against, and
  `val pairs: Map[String, Int] = m.withFilter(p).map { case (k, v) => k -> v }`
  -- which scalac accepts -- becomes `found: Iterable[(String, Int)]`.
* **attach, then roll back what turns out not to be a collection** looks
  equivalent and is not, because `attach_parents` marks the class done in
  `self.parented`: taking the parents away while leaving the mark is worse than
  never attaching them, since the lazy path a member lookup runs then finds the
  class already parented and does nothing. `object SortedSet extends
  SortedIterableFactory.Delegate[SortedSet]` is the case that says so, and
  `SortedSet.empty(ord)` became `value empty is not a member of SortedSet$` in
  four lines. Deciding up front has no such half; the fixture keeps that line
  anyway, because it is the shape the mistake was made on.

The element and the `CC` this slice is about are read off a collection's base
type, so a nested collection is exactly the family that needs the hierarchy --
and it is the family whose symbol identity a program can observe.

3. **`elem_type` and `rebuild_from_receiver` guessed.** With the hierarchy in
   place, `Check::elem_type` still answered `args[0]` for the element and
   `rebuild_from_receiver` still put `GroupedIterator` back as the `CC`, so
   `it.sliding(n).map(f)` reported `found: (Seq[A]) => B  required: (A) => Any`
   for a function that is exactly right, and `it.grouped(n).map(_.size)` came
   back as a `GroupedIterator[Int]` — a type claiming elements of `Seq[Int]`
   for a value whose elements are `Int`. Both now read the receiver's base type
   at `IterableOnce`: the element is the argument it passes there, and a class
   "maps to its own class" only when the element it passes is its *own* type
   parameter (or, for a `Map`, the pair of them). `Vector`, `TreeMap` and every
   other real collection are unchanged by construction, and both are pinned as
   controls in the fixture.

(3) is what the library's other seven lines are: `IntMap[T]` and `LongMap[T]`
are `IterableOnce[(Int, T)]` and `IterableOnce[(Long, T)]`, so
`m.foreach(kv => …)` was `found: ((Int, T)) => U  required: (T) => Any`
(`IntMap.scala:218`, `LongMap.scala:213`), and `Iterable.scala` 481/524 are
`grouped`/`sliding` seen from inside `IterableOps` itself.

### Cost

`elem_type` and `maps_to_own_class` now consult `base_type_args`, which
`docs/scala-library.md` records as a hot path. They run per *call site* of
`map`/`flatMap`/`foreach`/`withFilter`, not per `subst_as_seen_from`, and
`class_reaches` gates the walk to receivers that really are an `IterableOnce`
— `Option`, `Future`, `Try` and cats' `Ops[F, A]` never reach it. Measured on
`src/library`, min of three alternating runs of `tests/scalalib_measure.sh`:
2.19 s user before, 2.35 s after, with means of 2.44 s and 2.43 s. The effect
is inside the run-to-run spread.

### Found and not fixed: `grouped` on a real collection

Four cats lines that *look* like the family above are a different root and are
untouched: `NonEmptyLazyList.scala:462`
(`found: (LazyList[A]) => …  required: (Iterable[A]) => Any`),
`NonEmptyVector.scala:357` (`required: (Seq[A]) => Any`),
`NonEmptySeq.scala:364` and `instances/stream.scala:64`. These are
`IterableOps.grouped(size): Iterator[C]` on an ordinary collection, with `C`
answered as a *base* of the receiver rather than the receiver — the shape the
brief described, on a receiver that has nothing to do with `GroupedIterator`.

**They do not reproduce outside the full cats run**, which is the useful part of
the measurement. `final class NEV[+A] private (val toVector: Vector[A]) extends
AnyVal { def grp(size: Int): Iterator[NEV[A]] = toVector.grouped(size).map(NEV.unsafe) }`
compiles, and so does `NonEmptyVector.scala` compiled *on its own* with the
measure's own flags and classpath (130 other errors, none of them this one).
So the receiver's shape depends on what else is in the run, and the next slice
on this should start by recording where `Vector`'s parent list comes from in
the full run rather than by minimising the expression.

## The last 27 (agent/catsrest)

Measured on `9cac778e` (+ `4ac7c31b`, which touches no typer code), with the
same flags as `tests/cats_measure.sh`:

| | before | after |
|---|---:|---:|
| cats (339, 1 skipped) | 27 / 17 files | **4 / 3** |
| scala library (538) | 383 / 107 | **358 / 102** |
| gitbucket (354) | 92 / 43 | 92 / 43 (same lines) |
| slick (184) | `errors=0 classes=1504` | `errors=0 classes=1504` |

Every root below was reduced to a standalone program and compared with
scalac 2.13.16 in both directions; the reductions are
`tests/fixtures/catsr_infer.scala` (runs, output identical to scalac's build)
and `tests/fixtures/catsr_bad.scala` (rejected on exactly the lines scalac
rejects), driven by `crates/cli/tests/catsr.rs`.

### Twelve roots, each its own mechanism

1. **A compound result against a base of one component** (`instances/
   either.scala:244`, `EitherT.scala:1050`, 2). `mentions_tparam` did not look
   inside `Type::Refined`, so `def std[A]: MonadError[Either[A, *], A] with
   Traverse[…]` looked closed in value position and `A` was never read off
   the expected `Monad[Either[E, *]]`; `collect_expected` then paired
   components with the expected type only by *identical* class. It now falls
   back to the first component that has the expected class as a base and says
   something (`Q2[List[Int]] with Q[List[A]]` against `P[List[String]]`
   reads `Q`). The three arms after it were unreachable and are gone.
   The brief's "alpha-equivalence of type lambdas" was not involved.
2. **The receiver's variable solved in the first clause reaches the second**
   (`IndexedStateT.scala:444`, `IndexedReaderWriterStateT.scala:748`, 2).
   *Not* a `Tuple2`/`(C, B)` identity problem: `first(fa).dimap(f)(_.swap)`
   solves `first`'s `C` from `f`, but only `param_tys`, `ret` and the receiver
   were substituted -- `fun.ty`, which `fill_defaults_and_implicits` reads the
   later clauses from, kept the open `C`, and the message compared two
   different `C`s printed alike.
3. **A type path whose head is a still-inferred member `val`**
   (`Nested.scala` 117/124/126/128, 4). A template types its aliases before
   its other signatures, so `val FG = F0.compose(G0); type Representation =
   FG.Representation` read `FG` as `<notype>`. `path_dependent_type` now runs
   the head's pending completion first (a genuine cycle still reports).
4. **An implicit-only alternative against the nullary one**
   (`Nested.scala:162`, 1). `Alternative[F].compose[G]` collapsed in value
   position to `MonoidK.compose[G]` because it is nullary. nsc compares the
   results through the implicit clause plus the owner-subclass point; nine
   scalac probes (`C4`..`C9`, `Sub`/`Base`, `Q`/`P`) fix the rule: the
   implicit one wins only on a strictly positive score, and a tie keeps the
   nullary one. `overload_member_types` is keyed by symbol, so the receiver
   must really have the implicit alternative as a member.
5. **A curried method as a function argument** (`Apply.scala:247`, 1). The
   scoring eta-shape flattened every clause (`(Boolean, A, A) => A`); it is
   curried now (`Boolean => (A, A) => A`).
6. **A by-name function parameter given a `Function1` subclass**
   (`ContT.scala` 51/58, 2). The function-view step of `unify_tparam_all`
   did not look under `=>`.
7. **A type member does not shadow an implicit term** (`IorT.scala`
   609/614, 2). `type F[x]` in an anonymous class hid the enclosing
   `implicit F: Monad[F0]` from the search; only term bindings shadow.
8. **An outer member is read through the outer class** (`Parallel.scala`
   106/109, 2). An unqualified member found in an enclosing template was
   substituted as seen from the *innermost* class, which does not derive from
   its owner, so `NonEmptyParallel`'s own `M` stayed in `parallel: M ~> F`.
9. **Function-typed scrutinees and parents in patterns** (`Kleisli.scala`
   74/79, 2). `case StrictConstFunction1(fb)` and `case run:
   StrictConstFunction1[?]` on a `run: A => F[B]`: neither the constructor
   pattern nor the typed pattern read a `Type::Function` scrutinee as its
   `FunctionN` class, and `base_type_instance` could not walk a parent stored
   as a function type.
10. **The expected type reaches a case-class `apply` and an annotated
    literal's body** (`Kleisli.scala:571`, 1). `proto_arg_type` answered
    nothing for a companion `apply`, and an annotated literal was typed with
    no expected type at all, so its branches met at `AnyRef`.
11. **Method values, and `Either`'s prelude stubs** (`ArrowChoice.scala:58`,
    `syntax/either.scala:332`, `EitherK.scala:60`, 3). A monomorphic method value (`identity[C]`) now unifies as its
    eta-expansion; a polymorphic one (`Ior.both`) still goes through
    `solve_eta_tparams` -- the first version of this took both and cost 104
    cats errors, which is why the gate exists. `Either.map` in the prelude was
    the stub `(B => Any)Either[A, B]` (right for an application, which
    `either_map_result` rewrites, wrong for `eab.map` as a value); it is
    `map[B1](f: B => B1): Either[A, B1]` now. `Left.apply` / `Right.apply`
    likewise took `Any` and are `apply[A, B](value: A|B)`, which -- with root 10's prototype
    for the `EitherK(…)` companion call -- is what lets `EitherK.scala:60`
    hand `rightc` its `F`.
12. **A polymorphic nullary argument solved through the expected type**
    (`Arrow.scala:46`, 1). `compose(swap, compose(first(fa), swap))`: an
    argument solution carrying the argument's own undetermined variables
    (`A := (X, Y)`) could not be overridden by the invariant expected type,
    because it does not *conform* until `X` and `Y` are solved; and the inner
    call had no prototype at all, because the outer formal still mentions an
    unsolved parameter. nsc's lenient `protoTypeArgs` (open parameters as
    wildcards) is now given to a nested *call* only, and a wildcard that
    leaks into the argument's own type (`leftWiden(rightFunctor.widen(fac))`
    came back `F[_, D]`) sends it back to the untyped retry. A *bare*
    undetermined solution is left to `solve_undet_result` as before.

### What is left (4)

* `FunctionK.scala:95` -- `FunctionKMacroMethods` lives in the file the
  measure holds out (`CATS_EXCLUDE`). A measurement artefact.
* `syntax/semigroupal.scala` 71/78 -- **an inner class of a generic class
  loses its outer's type arguments.** `class B[T] { def m[A](a: A) = new
  B1(a); class B1[A0](a0: A0) { def n(z: T) = … } }` and then `new
  B[X].m(1).n(x)` is `found: X  required: T` (scalac accepts; five variants in
  the slice's probes). `Type::Class` has no prefix, so `B[X]#B1[A]` is plain
  `B1[A]` and `T` is never substituted. This is the same representation gap
  `docs/language-support.md` records for `A#B`; it needs a prefix on class
  types, not an inference rule.
* `NonEmptySet.scala:418` -- `found: SortedSet[A]  required: Iterable[A]`;
  does not reproduce standalone (`def f[A](s: SortedSet[A]): Iterable[A] =
  s` compiles), the base-type-merge family recorded above.

### Corpus

`CORPUS_KINDS=neg CORPUS_SIZE=full` against `corpus-c0c10f08.tsv`: three
tests went from pass to fail -- `abstract-class-2`, `t1010`,
`compile-time-only-a`. All three passed only on `type … is not a member of
<notype>`, the false error root 3 removes; scalac rejects them for reasons
this compiler does not check (a path-dependent prefix mismatch in the first
two, `@compileTimeOnly` in the third), so they are now accepted.

**`@compileTimeOnly` is implemented** (`crates/typer/src/compile_time_only.rs`),
so `compile-time-only-a` is rejected again, now on exactly the 26 lines its
`.check` lists. As in nsc's refchecks: a term reference to an annotated
symbol, and a *written* type naming one (`val v: (C7, C7)`, `new C1`,
`List.empty[C7]`, `x: @placebo`, `@placebo class Test`), is an error with the
annotation's message, except inside a definition that is itself annotated;
an annotated case class is reported at its definition (its companion's
`apply` names it) and a call to that `apply` is a reference; `List[C7]()` is
not, because nsc rewrites it to `Nil` first; an alias `type Al = K` is a
reference to `K` only where the alias is written. The annotation reaches an
implicit class's conversion and a `val` parameter's accessor, as its
meta-annotations say. Two things had to be fixed on the way: the parser read
`class O2` + newline + `@ann object O2` as a constructor annotation of the
class (nsc's scanner ends the header there), and nothing else changes for a
program without the annotation -- the check is off unless a source mentions
it, and pickled members carry no annotations here, so the library's own
placeholders are untouched. `tests/fixtures/catsr_cto_bad.scala` pins the
line set against scalac in both directions.

`abstract-class-2` and `t1010` need prefix-sensitive class types and were
handed to a separate agent. What the representation does today: `p.S1` for a
class `S1` is `path_dependent_type` -> `project_from_prefix` ->
`projected_class_type`, which answers `Type::Class { sym: S1, args: [] }`
(wrapped in the `AS_SEEN_FROM_MARK` refinement view only when the prefix
settles abstract members); `at_term_path` / `SymbolTable::path_member` keep
a path only for a *deferred type member* whose path starts at a local or a
self alias (`stable_term_path` refuses a class-member head such as `private
val in = new MailBox`, whose `in.Message` is therefore plain `Message`).
`Type::Class` has no prefix field, so `P.this.p.S1` and `P2.this.S1`, and
`MailBox#Message` and `in.Message`, are the same type to conformance and to
override matching alike.

### Composed with batch/w1: an abstract constructor applied invariantly

Merged onto `24cf81c1` (batch/w1 + prefix-carrying types), `catsr_bad`'s
`ArrowBad.bad1` -- `compose(swap, compose(first(fa), swap))` at a wrong
declared result, which scalac rejects -- compiled. Neither of root 12's two
mechanisms was involved (switching both off changed nothing). The outer
`compose` reads `B := (C, B)` from one argument and `B := (B, C)` from the
other, lubs them to `AnyRef`, and the `Applied`/`Applied` arm of
`is_sub_type` then accepted `F[(C, B), (B, C)]` for `F[AnyRef, (B, C)]`: that
arm, unchanged since higher-kinded types were first implemented, compared
every argument covariantly. `def up[F[_, _]](x: F[(Int, Int), Int]): F[AnyRef,
Int] = x` was accepted the same way.

When the constructor is an abstract type *parameter* whose own parameters
are known, each argument is now compared at the variance they declare
(`F[_]` invariant, `F[+_]` covariant, `F[-_]` contravariant); a wildcard on
either side is still containment, and every other constructor shape (a
partially applied class, a type lambda, a type member) keeps the covariant
reading. `catsr_bad`'s `InvariantCtor` pins both directions against scalac.

The companion fix is in `solve_eta_tparams`: nsc solves an eta-expansion's
parameters and result together, so the expected result's invariant positions
now go through `add_expected_constraints`. `F.pure[A]` against `B => F[BB]` is
`A := BB` (cats `Ior.to:485`, `OptionT.getOrElseF:302`), not the `A := B` the
parameter alone gives -- a `B => F[B]` an invariant `F` refuses. Without it the
corrected arm rejected those two valid cats files.

**Two neg tests lose a wrong-reason pass.** `neg/t7872b` and `neg/t7872c`
write `def up[F[+_]](fa: F[String]): F[Object] = fa` and `def down[F[-_]](fa:
F[Object]): F[String] = fa`. Under the covariant arm those *definitions* did
not typecheck (`F[Object] <: F[String]` asked `Object <: String`), which is the
error the corpus counted; read at the declared variance they are correct, and
both files now compile. What scalac rejects them for is not implemented:
`t7872c` needs nsc's `checkKindBounds` variance half -- an inferred `F := List`
does not match a `F[-_]` parameter ("type A is covariant, but type _ is
declared contravariant") -- and `t7872b` needs variance validation of a type
lambda's own body (`[-a]List[a]` uses `a` covariantly). Both are separate
checks, and the second is not about kind conformance at all; neither is a
reason to keep reading an abstract constructor covariantly.

**Both checks exist now** (`agent/kindvar`, `crates/typer/src/kind_bounds.rs`).
nsc's `checkKindBoundsHK` is run from the three bounds checks -- written and
inferred method type arguments, and the arguments of a written class type --
comparing a type argument's parameters with the higher-kinded parameter's
level by level: the same number, matching variance (an invariant expectation
accepts anything; the comparison direction alternates with every level, so
`M[_[_]]` refuses `CFunctor[F[+_]]` and `M[_[+_]]` accepts `Functor[F[_]]`),
and no stricter bounds. `Any` and `Nothing` are kind-overloaded, a wildcard
argument is skipped, and a bound that still mentions a type parameter after
instantiation is not judged. The message is nsc's, prefix and explanation
lines included, and `t7872c` is rejected on it. The variance of a type
lambda's own parameters is validated wherever the refinement is written
(`of value <local l>`), and of a named higher-kinded alias or abstract member
(`of type l`; a member's own parameter in its lower bound keeps its position,
the class's parameters flip there and are invariant in an alias's right-hand
side). `t7872` and `t7872b` are rejected on exactly scalac's lines.

Two things the probes turned up on the way. `FunctionN`'s hand-built class
symbols had no variance at all (`prelude_variance.rs` lists the prelude's
classes and had `TupleN` but not `FunctionN`), which the kind check would have
turned into a false refusal of `fn[Function1]` for `F[-_, +_]`; they are
`[-T1, …, -Tn, +R]` now. And slick's `HCons` writes `type Self = HCons[H @uv,
T @uv]` with `uncheckedVariance` renamed by its import; `Type::Annotated`
stored the written name, so the variance check did not recognise it and the
class-parameter half of the new member check rejected the file. The written
name is resolved at construction and stored as the annotation's own.

The full corpus found two more, both false refusals. `pos/t2994a` writes
`curry[m#a, s]` for `trait curry[n[_[_], _], s[_]]`, where `m#a`'s parameters
are bounded (`z <: NAT`) and `curry`'s are not, and scalac accepts it. Not
because class type applications escape the bounds half of the check: a plain
`type f = curry[C]` for `class C[z <: NAT]` *is* rejected by scalac
("type z's bounds <: NAT are stricter than type _'s declared bounds"), as is
the same `C` as a method's type argument. Thirteen probes drew the line where
nsc's trees draw it, not where the language does: an application that is an
argument of an abstract higher-kinded member's application (t2994a's
`n#a[curry[m#a, s]#f, z]`), or written in a method body with top-level
classes, is never visited by refchecks' bounds walk, while a local alias over
local classes is. Copying that would be copying an accident, so a class type
application's arguments are compared for arity and variance only; method
type arguments keep the full comparison. `pos/t8708` is a
separate-compilation test: `class X[+A]` from an earlier round arrives from
`-cp` with a shallow signature whose parameters are all invariant until its
pickle is adopted (`ensure_java_loaded`), and the variance checks read the
variances directly -- the pre-existing member check (`def m: X[B]` in a class
covariant in `B`) had the same false positive. The checks complete such a
class before reading it.

### Found in passing, not fixed

* `Option.map` (and the other hand-written collection `map`s in the prelude)
  are the same monomorphic stub `Either.map` was: `that.flatMap(o.map)` on an
  `Option` is `found: Option[B]  required: Option[C]` where scalac accepts.
* `new Q2[List[Int]] with Q[List[A]] {}` for `Q2[X] extends P[X]`, `Q[X]
  extends P[X]` is accepted; scalac reports that it "inherits different type
  instances of trait P".
* Without `-Xsource:3`, `ov(ite)` for an overloaded `ov` taking function
  types is accepted; scalac reports "missing argument list for method ite".
