// A generic local val keeps the result of a try/catch. The failed branch has
// an unconstrained covariant type parameter (`Future.failed`), so its lower
// bound is `Nothing`; the try expression must not widen the successful
// `Future[T]` branch to `Future[AnyRef]`.

import scala.concurrent.{ExecutionContext, Future}

object Main {
  implicit val ec: ExecutionContext = ExecutionContext.parasitic

  def use[T](f: Int => Future[T]): Future[T] = {
    def tryClose(): Unit = ()
    val fut =
      try f(1)
      catch { case t: Throwable => Future.failed(t) }
    fut.andThen { case _ => tryClose() }
  }

  def main(args: Array[String]): Unit = {
    println(use(_ => Future.successful(21)).value.get.get)
  }
}
