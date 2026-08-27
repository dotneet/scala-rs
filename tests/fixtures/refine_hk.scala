class Id[A](val value: A)
trait M { type F[_]; def wrap(x: Int): F[Int] }
class C extends M {
  type F[X] = Id[X]
  def wrap(x: Int): F[Int] = new Id(x)
}
object Main {
  def use(m: M { type F[X] = Id[X] }): Int = m.wrap(41).value
  def main(args: Array[String]): Unit = {
    println(use(new C))
    println(use(new C) - 39)
  }
}
