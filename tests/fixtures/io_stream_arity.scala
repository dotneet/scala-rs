import cats.effect.IO
import fs2.Stream
import scala.concurrent.duration.Duration
import java.util.concurrent.TimeUnit

object IOStreamArity {
  def materialize[T](p: Stream[IO, T]): IO[Vector[T]] =
    p.compile.toVector

  def foreach[T](p: Stream[IO, T])(f: T => Any): IO[Unit] =
    p.evalMap(t => IO(f(t))).compile.drain

  def materializeAsync[T, R](p: Stream[IO, T],
                             tr: T => IO[R],
                             delay: Duration = Duration(100L, TimeUnit.MILLISECONDS)): IO[Vector[R]] = {
    p.evalMap { t =>
      IO.sleep(scala.concurrent.duration.FiniteDuration(delay.toMillis, TimeUnit.MILLISECONDS)) >>
      tr(t)
    }.compile.toVector
  }
}
