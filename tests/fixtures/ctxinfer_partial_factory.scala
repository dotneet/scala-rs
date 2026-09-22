import cats.Monad
import cats.data.EitherT
import cats.implicits._

final class Service[F[_]: Monad] {
  def resolve(value: EitherT[F, String, Option[Int]]): EitherT[F, String, Int] =
    value.leftMap(e => e).flatMap(option => EitherT.fromOption(option, "missing"))
      .flatTap(n => EitherT.cond(n > 0, (), "negative"))
}

object Main extends App {
  final class Fixed[A, B](val value: B)
  def infer[A](f: Int => Fixed[String, Option[A]]): Fixed[String, Option[A]] = f(1)
  val nested = infer(n => new Fixed[String, Option[Int]](Some(n)))
  val service = new Service[Option]
  println(service.resolve(EitherT.fromEither[Option](Right(Some(42)))).value)
  println(service.resolve(EitherT.fromEither[Option](Right(None))).value)
  println(service.resolve(EitherT.fromEither[Option](Right(Some(-1)))).value)
  println(nested.value)
}
