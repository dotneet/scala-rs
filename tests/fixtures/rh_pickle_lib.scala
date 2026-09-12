// The library half of the pickle round trip: compiled by scala-rs, then read by
// real scalac (see `crates/cli/tests/rh.rs`). Every declaration here is a shape
// whose `ScalaSignature` we used to write in a way no reader could use, even
// though the classfiles ran perfectly.
//
//  * `Val2[E, *]` is a **type lambda** (kind-projector's `*`, which we implement
//    natively behind `-Ykind-projector`). We wrote it as a parentless
//    `ALIASsym`, so scalac saw an opaque `Λ$0$0 (in <none>)` and every implicit
//    instance for a partially applied type was invisible: `Functor[Either[E, *]]`,
//    `Applicative[Validated[E, *]]`, `Monad[OptionT[F, *]]`, …
//  * `Good` / `Pair` are **case classes**: the synthetic `apply`'s parameters were
//    pickled from the *constructor's* symbols, whose types still name the class's
//    type parameters, so `Good(2)` was `found: Int(2), required: A`.
//  * `ApOps` is a **value class** whose method has three parameter clauses. nsc
//    declares `map2$extension[B, C, F[_], A]($this)(fb)(f)(F)` -- the class's type
//    parameters *last*, the clause structure kept -- and asserts if what it finds
//    does not normalize to the original. Getting either wrong made scalac *crash*
//    on any client that called it (`no extension method found for: method map2`).
//  * `FK.lift` takes an **existential** function type. Written as a raw
//    `Function1`, scalac could not eta-expand a polymorphic method into it.
package rhpickle

trait Fun[F[_]] {
  def fmap[A, B](fa: F[A])(f: A => B): F[B]
}

sealed abstract class Val2[+E, +A]
final case class Bad[+E](e: E) extends Val2[E, Nothing]
final case class Good[+A](a: A) extends Val2[Nothing, A]
final case class Pair[A, B](a: A, b: B)

object Val2 {
  implicit def funForVal2[E]: Fun[Val2[E, *]] =
    new Fun[Val2[E, *]] {
      def fmap[A, B](fa: Val2[E, A])(f: A => B): Val2[E, B] = fa match {
        case Bad(e)  => Bad(e)
        case Good(a) => Good(f(a))
      }
    }
}

trait Ap[F[_]] { def ap2[A, B, C](fa: F[A], fb: F[B])(f: (A, B) => C): F[C] }

final class ApOps[F[_], A](private val fa: F[A]) extends AnyVal {
  def map2[B, C](fb: F[B])(f: (A, B) => C)(implicit F: Ap[F]): F[C] = F.ap2(fa, fb)(f)
  def tag: String = "ops"
}

trait ApSyntax {
  implicit final def apSyntaxOps[F[_], A](fa: F[A]): ApOps[F, A] = new ApOps(fa)
}

object all extends ApSyntax {
  implicit val listAp: Ap[List] = new Ap[List] {
    def ap2[A, B, C](fa: List[A], fb: List[B])(f: (A, B) => C): List[C] =
      fa.flatMap(a => fb.map(b => f(a, b)))
  }
}

trait FK[F[_], G[_]] { def apply[A](fa: F[A]): G[A] }

object FK {
  def lift[F[_], G[_]](f: (F[α] => G[α]) forSome { type α }): FK[F, G] =
    new FK[F, G] {
      def apply[A](fa: F[A]): G[A] = f.asInstanceOf[F[A] => G[A]](fa)
    }
}
