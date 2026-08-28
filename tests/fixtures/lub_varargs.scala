sealed trait Shape
case class Circle(r: Int) extends Shape
case class Rect(w: Int, h: Int) extends Shape
object Main {
  def area(s: Shape): Int = s match {
    case Circle(r) => 3 * r * r
    case Rect(w, h) => w * h
  }
  def main(args: Array[String]): Unit = {
    val shapes = List(Circle(1), Rect(2, 3))
    shapes.foreach(s => println(area(s)))
  }
}
