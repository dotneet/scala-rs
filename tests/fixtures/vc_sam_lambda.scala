package vcsam

final class Wrapped(val value: String) extends AnyVal

trait Eq[A] {
  def eqv(x: A, y: A): Boolean
}

object Main {
  val eqw: Eq[Wrapped] = (x: Wrapped, y: Wrapped) => x.value == y.value
  val f: Wrapped => String = (x: Wrapped) => x.value

  def main(args: Array[String]): Unit = {
    val x = new Wrapped("x")
    println(eqw.eqv(x, x))
    println(f(x))
  }
}
