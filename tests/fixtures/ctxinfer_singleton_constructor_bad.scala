import scala.concurrent.{Await, Future}
import scala.concurrent.duration.Duration

object Main {
  def collect[G[_], B](f: Int => G[B]): G[B] = f(1)
  val result = collect { _ =>
    val future = Future.successful(42)
    Await.ready(future, Duration.Inf)
  }
  val wrong: Future[String] = result
}
