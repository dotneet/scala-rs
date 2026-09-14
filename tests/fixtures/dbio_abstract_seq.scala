trait NoStream
trait Effect
object Effect { trait Read extends Effect; trait Write extends Effect }
trait Action[+R,+S <: NoStream,-E <: Effect]
trait FixedAction[+R,+S <: NoStream,-E <: Effect] extends Action[R,S,E]
import scala.collection.Factory
trait DBIOComponent {
  type ProfileAction[+R,+S <: NoStream,-E <: Effect] <: Action[R,S,E]
}
object DBIO {
  def sequence[R, M[+_ ] <: IterableOnce[_], E <: Effect](in: M[Action[R, NoStream, E]])(implicit cbf: Factory[R, M[R]]): Action[M[R], NoStream, E] = null
  def seq[E <: Effect](actions: Action[_,NoStream,E]*): Action[Unit,NoStream,E] = null
}
object Main extends DBIOComponent {
  val read: ProfileAction[Int,NoStream,Effect.Read] = (null: Any).asInstanceOf[ProfileAction[Int,NoStream,Effect.Read]]
  val fixed: FixedAction[String,NoStream,Effect.Write] = (null: Any).asInstanceOf[FixedAction[String,NoStream,Effect.Write]]
  val xs = DBIO.sequence(Vector(read))
  val mixed = Vector(read, fixed)
  val ys = DBIO.seq(read, fixed)
}
