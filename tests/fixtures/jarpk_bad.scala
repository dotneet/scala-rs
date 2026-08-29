// The kinds recovered from a pickle must still be *checked*: reading them is
// only worth anything if a wrong one is an error.
trait Monadic2[F[_]] {
  def pure[A](a: A): F[A]
}

object BadMain {
  // `Int` is not a type constructor.
  def wrongKind(m: Monadic2[Int]): Int = 0

  // `F.pure(1)` is `F[Int]`, not `F[String]`.
  def wrongResult[F[_]](implicit F: Monadic2[F]): F[String] = F.pure(1)

  def main(args: Array[String]): Unit = println("unreachable")
}
