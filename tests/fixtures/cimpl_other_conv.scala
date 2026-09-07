// Two conversions in scope supply `describe` for a `Bag[Int]`, and they are
// equally specific -- both take `F[A]` -- so nothing but their implicit
// clauses can separate them. `Show[Bag]` exists and `FlatMap[Bag]` does not,
// so the search drops `toFlatMapOps` and `toShowOps` wins outright rather than
// the two tying and the member being reported missing.

trait FlatMap[F[_]] {
  def flatMap[A, B](fa: F[A])(f: A => F[B]): F[B]
}

trait Show[F[_]] {
  def show[A](fa: F[A]): String
}

final class Bag[A](val a: A)

object Bag {
  implicit val showForBag: Show[Bag] = new Show[Bag] {
    def show[A](fa: Bag[A]): String = "Bag(" + fa.a + ")"
  }
}

final class FlatMapOps[F[_], A](val fa: F[A]) {
  def describe(implicit F: FlatMap[F]): String = "flatMap"
}

final class ShowOps[F[_], A](val fa: F[A]) {
  def describe(implicit S: Show[F]): String = S.show(fa)
}

object syntax {
  implicit def toFlatMapOps[F[_], A](fa: F[A])(implicit F: FlatMap[F]): FlatMapOps[F, A] =
    new FlatMapOps(fa)
  implicit def toShowOps[F[_], A](fa: F[A])(implicit S: Show[F]): ShowOps[F, A] =
    new ShowOps(fa)
}

import syntax._

object Main {
  def main(args: Array[String]): Unit = {
    println(new Bag(7).describe)
    println(new Bag("ab").describe)
  }
}
