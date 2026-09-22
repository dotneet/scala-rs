import cats.data.EitherT
import cats.implicits._
import cats.instances.future._
import scala.concurrent.{ExecutionContext, Future}

final case class MissingResource() extends RuntimeException

object Main {
  implicit val ec: ExecutionContext = ExecutionContext.global

  def run(
      first: EitherT[Future, IllegalArgumentException, Int],
      lookup: Int => Future[Option[Int]]
  ): Future[Int] =
    (for {
      id <- first.leftMap(_ => new IllegalArgumentException())
      value <- EitherT.fromOptionF(lookup(id), MissingResource())
        .leftMap(_ => new RuntimeException())
    } yield value).value.map(_.fold(throw _, identity))
}
