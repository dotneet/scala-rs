sealed trait Effect
trait Read extends Effect
trait Write extends Effect

final class Action[+R, -E <: Effect](val value: R) {
  def flatMap[R2, E2 <: Effect](f: R => Action[R2, E2]): Action[R2, E with E2] =
    f(value)

  def map[R2](f: R => R2): Action[R2, E] =
    new Action[R2, E](f(value))
}

object Main {
  def read[A](value: A): Action[A, Read] = new Action[A, Read](value)
  def write(value: Int): Action[Int, Write] = new Action[Int, Write](value)

  val combined: Action[Int, Read with Write] =
    for {
      first <- read(1)
      second <- read(first + 1)
      result <- write(second + 1)
    } yield result

  def main(args: Array[String]): Unit = println(combined.value)
}
