package slickstreaming

trait NoStream
trait Streaming[+T] extends NoStream
trait Action[+R, +S <: NoStream, -E]
trait Fixed[+R, +T, -E] extends Action[R, Streaming[T], E]

trait Api {
  def action: Fixed[Seq[Int], Int, Nothing]
}
