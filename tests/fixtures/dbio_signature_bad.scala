package dbio_signature

object BadUse {
  // An effect intersection cannot be widened to only E: Action's effect
  // parameter is contravariant.  This must remain an error for a consumer.
  def flat[R, S <: NoStream, E <: Effect, R2, S2 <: NoStream, E2 <: Effect](
      a: Action[R, S, E],
      f: R => Action[R2, S2, E2]
  ): Action[R2, S2, E] = a.flatMap(f)

  def thenDo[R, S <: NoStream, E <: Effect, R2, S2 <: NoStream, E2 <: Effect](
      a: Action[R, S, E],
      b: Action[R2, S2, E2]
  ): Action[R2, S2, E] = a.andThen(b)
}
