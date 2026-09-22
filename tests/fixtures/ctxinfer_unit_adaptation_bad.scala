import scala.concurrent.{ExecutionContext, Future}

object Main {
  implicit val executionContext: ExecutionContext = ExecutionContext.global

  class Message
  final class TextMessage extends Message

  def offer(message: Message): Future[Int] = Future.successful(1)
  def install(callback: TextMessage => Unit): Unit = callback(new TextMessage)

  val raw = offer _
  install(raw)
}
