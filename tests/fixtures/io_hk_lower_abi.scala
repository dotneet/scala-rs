class HC[F[_]] {
  def lower[F2[_] >: F[_], A](a: A): F2[A] = ???
}

object IOHkLowerAbi {
  def inferred(c: HC[List]) = c.lower(1)
}
