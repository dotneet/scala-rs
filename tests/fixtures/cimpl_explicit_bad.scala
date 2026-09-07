// Written out by hand, the conversion is an ordinary call and its implicit
// clause is an ordinary clause: nsc reports the missing witness here, and so
// must we. Discarding a view is a rule of the *search*, not a licence to
// swallow the diagnostic.

trait FlatMap[F[_]] {
  def flatMap[A, B](fa: F[A])(f: A => F[B]): F[B]
}

final class Bag[A](val a: A)

final class FlatMapOps[F[_], A](val fa: F[A]) {
  def flatMap[B](f: A => F[B])(implicit F: FlatMap[F]): F[B] = F.flatMap(fa)(f)
}

object syntax {
  implicit def toFlatMapOps[F[_], A](fa: F[A])(implicit F: FlatMap[F]): FlatMapOps[F, A] =
    new FlatMapOps(fa)
}

import syntax._

object Main {
  def main(args: Array[String]): Unit = {
    println(toFlatMapOps(new Bag(1)).fa.a)
  }
}
