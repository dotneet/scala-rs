// The leftover type parameter of an eta-expansion is a variable, and a
// variable is still bounded.
//
// `Inv` is invariant in both parameters, so a leftover minimised to `Nothing`
// cannot slip through by covariance the way `Ior`'s can: the enclosing
// expected type really does say `B = String`, and `String` is not a `Number`.
// scalac 2.13.16 reports `type arguments [A,String] do not conform to method
// make's type parameter bounds [A,B <: Number]`.
//
// In a file of its own because nsc stops reporting this one as soon as the
// same object carries another type error, which would hide it.

final case class Inv[A, B](a: A)
object Inv {
  def make[A, B <: Number](a: A): Inv[A, B] = Inv(a)
}

trait Functor[F[_]] { def map[A, B](fa: F[A])(f: A => B): F[B] }

object Bad {
  def outOfBounds[F[_], A](fa: F[A])(implicit F: Functor[F]): F[Inv[A, String]] =
    F.map(fa)(Inv.make)

  // The enclosing method binds a `B` of its own, so found and required would
  // print the same string. This is the shape cats' `IorT` reported, and the
  // diagnostic has to say which `B` is which.
  def shadowed[F[_], A, B](fa: F[A])(implicit F: Functor[F]): F[Inv[A, B]] =
    F.map(fa)(Inv.make)
}
