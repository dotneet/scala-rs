class Id[A](val value: A)
trait M { type F[_] }
class C extends M {
  type F[X] = Id[X]
  def wrap(x: Int): F[Int] = new Id(x)
}
object Main {
  def main(args: Array[String]): Unit = {
    val c = new C
    val x: c.F[Int] = c.wrap(41)
    println(x.value)
    val y: c.F[Int] = new Id(2)
    println(y.value)
  }
}
