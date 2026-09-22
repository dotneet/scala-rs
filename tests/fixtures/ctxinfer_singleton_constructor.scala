import scala.concurrent.{Await, Future}
import scala.concurrent.duration.Duration

trait Witness[F[_]]
object Witness {
  implicit val future: Witness[Future] = new Witness[Future] {}
}

object Main extends App {
  def collect[G[_], B](f: Int => G[B])(implicit w: Witness[G]): G[B] = f(1)

  val result = collect { _ =>
    val future = Future.successful(42)
    Await.ready(future, Duration.Inf)
  }
  val check: Future[Int] = result
  println(Await.result(check, Duration.Inf))
}
