// Each of these is an error nsc 2.13.16 gives for the same source: the new
// inference must not make any of them go away.

trait FlatMap[F[_]] {
  def flatMap[A, B](fa: F[A])(f: A => F[B]): F[B]
  def pure[A](a: A): F[A]
}

final case class Cell[+F <: Option[Int]](name: String, f: F = None)

object Bad {
  // `F[Int]` really is the result; `F[String]` is not.
  def wrongResult[F[_]](fa: F[Int])(implicit F: FlatMap[F]): F[String] =
    F.flatMap(fa)(i => F.pure(i))

  // The lambda's parameter is the element of `fa`, not a `String`.
  def wrongParam[F[_]](fa: F[Int])(implicit F: FlatMap[F]): F[Int] =
    F.flatMap(fa)((s: String) => F.pure(s.length))

  // `copy` re-infers `F`, but only within the class's own bound.
  def badCopy(c: Cell[Some[Int]]): Cell[Option[Int]] = c.copy(name = 3)
}
