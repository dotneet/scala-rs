// scalac: case class B has case ancestor Main.A, but case-to-case
// inheritance is prohibited.
object Main {
  case class A(x: Int)
  case class B(y: Int) extends A(y)
  def main(args: Array[String]): Unit = println(B(1))
}
