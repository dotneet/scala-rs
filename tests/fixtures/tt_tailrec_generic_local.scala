import scala.annotation.tailrec

class TailrecGenericLocal {
  def run[A, B](a: A)(f: A => Either[A, B]): B = {
    @tailrec
    def loop(next: A): B = f(next) match {
      case Right(value) => value
      case Left(nextValue) => loop(nextValue)
    }
    loop(a)
  }
}

object Main {
  def main(args: Array[String]): Unit = {
    println(new TailrecGenericLocal().run(0)(n => if (n < 100000) Left(n + 1) else Right(n)))
  }
}
