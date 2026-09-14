// A DBIOAction.seq-shaped varargs call has to infer the greatest lower bound
// of all contravariant effects.  With Read and Write arguments, E is
// Read with Write, so both arguments conform to Action[..., E].

trait NoStream
trait Effect

object Effect {
  trait Read extends Effect
  trait Write extends Effect
}

trait Action[+R, +S <: NoStream, -E <: Effect] {
  def result: R
}

object Action {
  def successful[R, E <: Effect](value: R): Action[R, NoStream, E] =
    new Action[R, NoStream, E] {
      def result: R = value
    }

  def seq[E <: Effect](actions: Action[_, NoStream, E]*): Action[Unit, NoStream, E] =
    successful[Unit, E](())
}

object Main {
  val write: Action[Unit, NoStream, Effect.Write] = Action.successful[Unit, Effect.Write](())
  val read: Action[Int, NoStream, Effect.Read] = Action.successful[Int, Effect.Read](1)

  // The direct call and a generic forwarding method exercise the same
  // lower-bound/intersection inference used by Slick's DBIOAction.seq.
  val direct = Action.seq(write, read)

  def forward[E <: Effect](actions: Action[_, NoStream, E]*): Action[Unit, NoStream, E] =
    Action.seq[E](actions: _*)

  val forwarded = forward(write, read)

  def main(args: Array[String]): Unit =
    println((direct ne null) && (forwarded ne null))
}
