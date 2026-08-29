// Widening the conversion's parameter to "any type constructor applied to one
// argument" must not make it apply to a type that has no witness: `Bag` has no
// `FlatMap` instance anywhere, so `bag.flatMap(…)` is still an error and the
// conversion must not be inserted.

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
    println(new Bag(1).flatMap(n => new Bag(n)).a)
  }
}
