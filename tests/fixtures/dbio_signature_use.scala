package dbio_signature

object Use {
  def flat[R, S <: NoStream, E <: Effect, R2, S2 <: NoStream, E2 <: Effect](
      a: Action[R, S, E],
      f: R => Action[R2, S2, E2]
  ): Action[R2, S2, E with E2] = a.flatMap(f)

  def thenDo[R, S <: NoStream, E <: Effect, R2, S2 <: NoStream, E2 <: Effect](
      a: Action[R, S, E],
      b: Action[R2, S2, E2]
  ): Action[R2, S2, E with E2] = a.andThen(b)
}
