package cats.evidence

abstract class Is[A, B] extends Serializable {
  def substitute[F[_]](fa: F[A]): F[B]
}

object Is extends IsSupport {
  def refl[A]: Is[A, A] = null
}
