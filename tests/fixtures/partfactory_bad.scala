object Bad {
  def missing[F[_], A](a: A): Wrap[F, A] = Wrap.pure(a)
}
