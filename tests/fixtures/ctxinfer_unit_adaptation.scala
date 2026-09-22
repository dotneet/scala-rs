import scala.concurrent.{Await, ExecutionContext, Future}
import scala.concurrent.duration._

object Main {
  implicit val executionContext: ExecutionContext = ExecutionContext.global

  final class Blob

  def store(): Future[Unit] = Future {
    new Blob
  }

  class Message
  final class TextMessage extends Message

  var offered = 0

  def offer(message: Message): Future[Int] = {
    offered += 1
    Future.successful(1)
  }

  def install(callback: TextMessage => Unit): Unit = callback(new TextMessage)

  def main(args: Array[String]): Unit = {
    Await.result(store(), 5.seconds)

    val explicit: TextMessage => Unit = offer _
    explicit(new TextMessage)
    install(offer _)
    install(offer)

    assert(offered == 3, s"expected three calls, found $offered")
    println("unit-adaptation-ok")
  }
}
