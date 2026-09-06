trait Alternative[F[_]]

trait Monad[F[_]] {
  def untilM[G[_], A](f: F[A])(cond: => F[Boolean])(implicit G: Alternative[G]): F[G[A]]
  def untilM_[A](f: F[A])(cond: => F[Boolean]): F[Unit]
}

object ByNameFollowup {
  trait Ops[F[_], A] {
    type TypeClassType <: Monad[F]
    def self: F[A]
    val typeClassInstance: TypeClassType
    def untilM[G[_]](cond: => F[Boolean])(implicit G: Alternative[G]): F[G[A]] =
      typeClassInstance.untilM[G, A](self)(cond)(G)
    def untilM_(cond: => F[Boolean]): F[Unit] = typeClassInstance.untilM_[A](self)(cond)
  }
}
