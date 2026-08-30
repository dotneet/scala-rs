// No stubbing: giving the lambda its declared parameter type does not make the
// call legal. `Bag` has no `FlatMap` instance, so the implicit clause of
// `Ops.flatMap` has no witness and the call is rejected -- the same error
// scalac reports.

trait FlatMap[F[_]] {
  def flatMap[A, B](fa: F[A])(f: A => F[B]): F[B]
}

final class Bag[A](val a: A)

final class Ops[F[_], A](val self: F[A]) {
  def flatMap[B](f: A => F[B])(implicit F: FlatMap[F]): F[B] = F.flatMap(self)(f)
}

object Main {
  def main(args: Array[String]): Unit = {
    println(new Ops[Bag, Int](new Bag(3)).flatMap(n => new Bag(n + 1)).a)
  }
}
