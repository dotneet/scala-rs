sealed trait Color
case class RGB(n: Int) extends Color
case object Black extends Color
object Main {
  def show(c: Color): Int = c match {
    case RGB(n) => n
  }
  def main(args: Array[String]): Unit = {
    println(show(RGB(3)))
  }
}
