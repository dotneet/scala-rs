// Without `-Xsource:3`, `A & B` is an ordinary infix type application and
// scalac reports `not found: type &`. We must diagnose it too, not accept it.
trait Named {
  def name: String
}
trait Aged {
  def age: Int
}

object Main {
  def show(x: Named & Aged): String = x.name

  def main(args: Array[String]): Unit = println(show(null))
}
