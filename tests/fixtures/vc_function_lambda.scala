package vcfn

final class Underlying(val text: String)
final class Wrapped(val value: Underlying) extends AnyVal

trait Eq[A] {
  def eqv(x: A, y: A): Boolean
}

object Eq {
  def by[A, B](f: A => B): Eq[A] = new Eq[A] {
    def eqv(x: A, y: A): Boolean = f(x) == f(y)
  }
}

object Main {
  val eqw: Eq[Wrapped] = Eq.by(_.value)

  def main(args: Array[String]): Unit = {
    val x = new Wrapped(new Underlying("x"))
    println(eqw.eqv(x, x))
  }
}
