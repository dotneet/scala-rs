trait Functor[F[_]] {
  def map[A, B](fa: F[A])(f: A => B): F[B]
}

object Main {}

final case class Kleisli[F[_], -A, B](run: A => F[B]) {
  def apply(a: A): F[B] = run(a)
}

trait ArrowChoice[F[_]] {
  implicit def F: Functor[F]

  def choose[A, B, C, D](f: Kleisli[F, A, C])(g: Kleisli[F, B, D]): Kleisli[F, Either[A, B], Either[C, D]] =
    Kleisli((fe: Either[A, B]) =>
      fe match {
        case Left(a)  => F.map(f(a))(Left.apply _)
        case Right(b) => F.map(g(b))(Right.apply _)
      }
    )
}
