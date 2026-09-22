import cats.Monad
import cats.data.EitherT
import cats.implicits._

final class Service[F[_]: Monad] {
  def resolve(value: EitherT[F, String, Option[Int]]): EitherT[F, String, Int] =
    value.flatMap(option => EitherT.fromOption[List](option, "missing"))
}
