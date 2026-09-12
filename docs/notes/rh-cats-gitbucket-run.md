# Running the cats and gitbucket code we emit (`agent/runharness`)

`tests/cats_run.sh` + `tests/catsrun/` and `tests/gitbucket_run.sh` +
`tests/gbrun/` are differential **execution** harnesses, built on the model of
`tests/slick_run.sh`: each compiles its project twice (scala-rs and real scalac
2.13.16), compiles a set of ordinary client programs with real scalac, and runs
them against both builds comparing stdout byte for byte. Each client is compiled
*both* ways, which separates a codegen defect (our classes run differently) from
a pickle defect (real scalac cannot use our `ScalaSignature` at all).

Before this slice nothing had executed one instruction of either project. Eleven
defects came out of the first runs; the ones below are what is left, each reduced
to a standalone program and paired with scalac's behaviour on the same source.

Every reduction is in this file rather than in a fixture because it is a root
this slice does not own (`ownership.md`) or one it stopped on. Both harnesses
carry the list as an *expected-failure ledger*: the program name, the reason, and
a check in both directions -- an unlisted failure fails the harness, and a listed
program that starts passing fails it too, so the list has to shrink.

---

## 1. A redundant mixin forwarder overrides a `final` trait accessor

Owner: **agent/mixcg** (mixin forwarders).
Harness: `tests/cats_run.sh Chains`.

cats' `CoreDurationInstances` declares `implicit final val
catsStdShowForDurationUnambiguous`. `AllInstancesBinCompat` is a *class* that
mixes it in, so it carries the field and a `final` accessor -- in scalac's output
and in ours, identically. `cats.instances.package$all$` then extends that class,
and **we emit the accessor a second time there**:

```text
scalac:  javap -p 'cats.instances.package$all$' | grep DurationUnambiguous   -> nothing
ours:    public final cats.Show catsStdShowForDurationUnambiguous();
         public final void cats$instances$CoreDurationInstances$_setter_$…_$eq(cats.Show);
```

so loading the class fails before any code runs:

```text
java.lang.IncompatibleClassChangeError: class cats.instances.package$all$
  overrides final method
  cats.instances.AllInstancesBinCompat.catsStdShowForDurationUnambiguous()Lcats/Show;
```

A mixin forwarder must not be emitted for a member an ancestor **class** already
implements. Reduced shape (not yet a fixture: it needs the three-level
trait/class/object chain with a `final val` in the trait):

```scala
trait T { implicit final val v: String = "v" }
class Base extends T          // carries the field and the final accessor
object Leaf extends Base      // must NOT declare `v()` again
```

## 2. `NonEmptySet`'s newtype ops are missing from our pickle

Owner: unclaimed; pickle/implicit-surface.
Harness: `tests/cats_run.sh Monoids`.

`cats.data.NonEmptySet[A]` is `NonEmptySetImpl.Type[A]`, an abstract type member
with `>: Base <: Base with Tag`, and every operation arrives through
`catsNonEmptySetOps` / the `NonEmptySetOps` value class. Real scalac reading our
cats says:

```text
error: value head is not a member of Type
error: value toSortedSet is not a member of Type
error: value map is not a member of Type
error: value ++ is not a member of Type
error: could not find implicit value for parameter ev: Semigroup[cats.data.NonEmptySet[Int]]
```

The classes themselves run: the same calls work when the client is compiled
against scalac's cats and run against ours. So this is the pickled *type* of the
newtype or of the ops conversion, not codegen. `NonEmptyChain` is the same
newtype shape and does not report this, which is the first place to look for the
difference.

## 3. A lambda inside a pattern-bound `for` loses its captured outer

Owner: unclaimed; lambda lifting / capture.
Harness: `tests/cats_run.sh NaturalTransforms`.

`cats/arrow/FunctionKMacros.scala` contains

```scala
for (typeArg @ TypeTree() <- typeArgs) if (typeArg.original != null) c.abort(...)
```

inside `class Lifter[C <: blackbox.Context](val c: C)`. The `withFilter`
predicate that desugars out of the pattern reads `c` off the enclosing `Lifter`,
so it captures `this`. We emit the body method with **one** parameter:

```text
ours:   public static java.lang.Object $anonfun$0(java.lang.Object);
          0: aload_0; 1: checkcast Trees$TreeApi; ...; 19: getfield Lifter.c
```

`aload_0` is the *element*, so the captured `Lifter` is never passed, and real
scalac running our macro classfile dies:

```text
java.lang.ClassCastException: class scala.reflect.internal.Trees$TypeTree
  cannot be cast to class cats.arrow.FunctionKMacros$Lifter
    at cats.arrow.FunctionKMacros$Lifter.$anonfun$0(FunctionKMacros.scala)
```

(Before this was reached, the same call failed twice for reasons now fixed: the
`forSome` parameter type pickled as a raw `Function1`, and a `getfield` on an
`Object`-typed captured receiver with no cast.)

Reduced shape to build a fixture from -- a pattern binding with an extractor in a
`for` over a generic collection, inside a class whose field the body reads:

```scala
class K(val tag: String) {
  def go(xs: List[Any]): List[String] =
    (for (s @ Some(_) <- xs) yield tag + s).map(_.toString)
}
```

## 4. `OptionT` instance ambiguity: the instance traits' order is not visible

Owner: unclaimed; pickle (linearization / specificity).
Harness: `tests/cats_run.sh Transformers`.

```text
error: ambiguous implicit values:
 both method catsDataMonadErrorMonadForOptionT in class OptionTInstances1
   of type [F[_]](implicit F0: Monad[F]): MonadError[…OptionT[F,β]…, Unit]
 and method catsDataMonadErrorForOptionT in class OptionTInstances0
   of type [F[_], E](implicit F0: MonadError[F,E]): MonadError[…OptionT[F,β]…, E]
 match expected type Monad[[β]cats.data.OptionT[[A]Either[String,A],β]]
```

cats resolves this tie by *owner specificity*: `OptionTInstances0` extends
`OptionTInstances1`, so its member wins. The same client compiles against
scalac's cats, so the relation between the two instance traits is not coming
through our pickle -- the parent list of the `OptionTInstances*` chain, or the
order it is written in, is the thing to check.

## 5. `returning … insert` infers the inserted id as `Nothing`

Owner: inference; closest listed owner is **agent/hardinfer** (a type parameter
that only the argument determines).
Harness: `tests/gitbucket_run.sh Utils`.

gitbucket's `AccountService.createAccount` ends with

```scala
val accountId = Accounts returning Accounts.map(_.accountId) insert account
account.copy(accountId = accountId)
```

`R` in `BlockingAPI.ReturningInsertActionComposer2[T, R]` is determined only by
the argument (`ReturningInsertActionComposer[T, R]`). We infer it as `Nothing`,
so the result is discarded and the `copy` becomes an unconditional throw:

```text
ours:    150: invokevirtual …ReturningInsertActionComposer2.insert:(…)Ljava/lang/Object;
         153: pop
         154: aload 11
         156: astore 12
         158: new gitbucket/core/model/Account
         161: dup
         162: athrow                       <- VerifyError: uninitialized on athrow
scalac:   79: invokevirtual …insert:(…)Ljava/lang/Object;
          82: invokestatic  BoxesRunTime.unboxToLong
          85: lstore_3 … 87: lreturn
```

Standalone reduction (`slick` 3.4.1 + `blocking-slick` 0.0.14 on the classpath;
both sides compile, only ours emits `pop; athrow`):

```scala
import com.github.takezoe.slick.blocking.BlockingH2Driver
import BlockingH2Driver.blockingApi._

object P18 {
  class Rs(tag: Tag) extends Table[(Long, String)](tag, "R") {
    def id = column[Long]("ID", O.AutoInc)
    def n = column[String]("N")
    def * = (id, n)
  }
  val Rs = TableQuery[Rs]
  def mk(implicit s: Session): Long = {
    val row = (0L, "x")
    val id = Rs returning Rs.map(_.id) insert row
    id
  }
}
```

`javap -p -c` on `mk` is the whole test: `unboxToLong; lstore; lreturn` against
`pop; athrow`.

## 6. `Database.forURL` is not found in a client that imports `blockingApi._`

Owner: unclaimed; member lookup through a profile's `api` cake.
Noticed while reducing 5, not covered by a harness program.

```scala
import com.github.takezoe.slick.blocking.BlockingH2Driver
import BlockingH2Driver.blockingApi._
val db = Database.forURL("jdbc:h2:mem:x", driver = "org.h2.Driver")
```

scalac accepts this; we report

```text
error: value forURL is not a member of BasicBackend.DatabaseFactory
```

`Database` resolves to `BasicBackend`'s factory rather than `JdbcBackend`'s. The
harness programs reach `Database.forURL` only through gitbucket's *own* compiled
code, so this does not block them -- but it would block any client of ours.
