// The same conversion with no witness anywhere. nsc discards the view during
// the search and the diagnostic that survives is the selection's own, at the
// selection's position -- not the missing `FlatMap[Bag]`, which the user never
// asked for.

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
