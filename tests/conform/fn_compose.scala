object Main {
  trait Box[+A] { def get: A }
  case class B[A](get: A) extends Box[A]
  def widen(b: Box[String]): Box[Any] = b
  def main(a: Array[String]): Unit = {
    println(widen(B("x")).get)
    val f: Int => Int = x => x + 1
    val g = f andThen (_ * 2) compose ((x: Int) => x - 1)
    println(g(5))
    println(List(1,2,3).reduce(_ + _))
    println((1 to 3).map(_.toString).reduceLeft(_ + _))
  }
}
