// Varargs element inference against the real scala-library: `List.apply[A]`
// takes the lub of every argument, so this is a `List[Shape]`.
sealed trait Shape
case class Circle(r: Int) extends Shape
case class Rect(w: Int, h: Int) extends Shape

object Main {
  def area(s: Shape): Int = s match {
    case Circle(r)  => r * r * 3
    case Rect(w, h) => w * h
  }

  def main(args: Array[String]): Unit = {
    val shapes = List(Circle(1), Rect(2, 3), Circle(4))
    shapes.foreach(s => println(area(s)))
    println(shapes.length)

    val more = Circle(5) :: shapes
    more.foreach(s => println(area(s)))

    // Homogeneous lists keep their exact element type.
    println(List(1, 2, 3).length)
    println(List("a", "b").length)
  }
}
