// Making the leftover type parameter of an eta-expansion a variable must not
// make it a wish: the solution the enclosing call finds still has to conform.
// scalac 2.13.16 rejects both of these.

sealed trait Ior[+A, +B]
object Ior {
  final case class Left[+A](a: A) extends Ior[A, Nothing]
  def left[A, B](a: A): Ior[A, B] = Left(a)
}

trait Functor[F[_]] { def map[A, B](fa: F[A])(f: A => B): F[B] }

final case class IorT[F[_], A, B](value: F[Ior[A, B]])

object Bad {
  // `Ior.left` gives `Ior[A, ?]`; the result asks for `Ior[B, A]`. Solving
  // the leftover cannot repair the first argument.
  def swapped[F[_], A, B](fa: F[A])(implicit F: Functor[F]): IorT[F, B, A] =
    IorT(F.map(fa)(Ior.left))

  // A fully concrete expected function type: the parameter disagrees, and no
  // variable is open to absorb it.
  val f: Int => Ior[String, Int] = Ior.left
}
