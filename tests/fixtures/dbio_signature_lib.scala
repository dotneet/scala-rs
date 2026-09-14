// The public portion of Slick's DBIOAction effect-carrying API.  The JVM
// Signature attribute can only carry the erased E return type; the ScalaSig
// pickle must retain `E with E2` for a downstream Scala compiler.
package dbio_signature

trait Effect
trait NoStream

trait Action[+R, +S <: NoStream, -E <: Effect] {
  def flatMap[R2, S2 <: NoStream, E2 <: Effect](
      f: R => Action[R2, S2, E2]
  ): Action[R2, S2, E with E2]

  def andThen[R2, S2 <: NoStream, E2 <: Effect](
      a: Action[R2, S2, E2]
  ): Action[R2, S2, E with E2]
}
