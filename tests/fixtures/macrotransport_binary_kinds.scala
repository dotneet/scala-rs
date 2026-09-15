import scala.concurrent.{Await, ExecutionContext, Future}
import scala.concurrent.duration.Duration
import cats.ApplicativeError
import io.circe.Encoder
import io.circe.syntax._

object Main {
  def encode[T: Encoder](value: T)(implicit ec: ExecutionContext): Future[String] = {
    val json = value.asJson
    implicitly[ApplicativeError[Future, Throwable]].pure(json.noSpaces)
  }
  def main(args: Array[String]): Unit = {
    implicit val ec: ExecutionContext = ExecutionContext.global
    println(Await.result(encode("ok"), Duration.Inf))
    import cats.implicits._
    val sequenced = List(Future.successful(42)).sequence
    println(Await.result(sequenced, Duration.Inf))
  }
}
