case class Box[+A](a: A) {
  def map[B](f: A => B): Box[B] = Box(f(a))
}
case class Pair[A, B](first: A, second: B)
object Main {
  def main(args: Array[String]): Unit = {
    val b = Box(1)
    println(b.a + 1)
    println(b.map(_ * 2))
    val wide: Box[Any] = b
    println(wide)
    val p = Pair("x", 3)
    println(p.first + p.second)
    println(p.copy(second = 9))
    b match { case Box(v) => println(v + 100) }
  }
}
