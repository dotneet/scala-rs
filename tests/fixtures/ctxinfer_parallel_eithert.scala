import cats.{Monad, Parallel}
import cats.data.EitherT
import cats.instances.lazyList._
import cats.instances.parallel._
import cats.syntax.all._

trait Problem
case object Failure extends Problem

object Main {
  def joined[F[_]: Monad: Parallel](
      a: EitherT[F, Problem, Int],
      b: EitherT[F, Problem, Int]
  ): EitherT[F, Problem, Int] = (a, b).parMapN(_ + _)

  def right[F[_]: Monad](value: Int): EitherT[F, Problem, Int] =
    EitherT.fromEither[F](Right(value): Either[Problem, Int])

  def left[F[_]: Monad]: EitherT[F, Problem, Int] =
    EitherT.fromEither[F](Left(Failure): Either[Problem, Int])

  def main(args: Array[String]): Unit = {
    println(joined[LazyList](right[LazyList](1), right[LazyList](2)).value.toList)
    println(joined[LazyList](left[LazyList], right[LazyList](2)).value.toList)
  }
}
