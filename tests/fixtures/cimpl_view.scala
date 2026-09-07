// A conversion whose parameter is `F[A]` fits every one-argument application
// by shape, so what decides whether it applies is its own implicit clause.
// Here the clause has a witness: `Bag` has a `FlatMap` on its companion, so
// `bag.flatMap(f)` goes through `toFlatMapOps` and runs.

trait FlatMap[F[_]] {
  def flatMap[A, B](fa: F[A])(f: A => F[B]): F[B]
}

final class Bag[A](val a: A)

object Bag {
  implicit val flatMapForBag: FlatMap[Bag] = new FlatMap[Bag] {
    def flatMap[A, B](fa: Bag[A])(f: A => Bag[B]): Bag[B] = f(fa.a)
  }
}

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
    println(new Bag(3).flatMap(n => new Bag(n + 1)).a)
    println(new Bag("x").flatMap(s => new Bag(s + "y")).a)
  }
}
