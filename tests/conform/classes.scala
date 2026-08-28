abstract class Shape(val name: String) {
  def area: Double
  override def toString: String = name + ":" + area
}
class Circle(r: Double) extends Shape("circle") { def area: Double = 3.0 * r * r }
class Square(s: Double) extends Shape("square") { def area: Double = s * s }
trait Tagged { def tag: String = "t" }
class Both extends Square(2.0) with Tagged { override def tag: String = "b" }
object Main {
  def main(args: Array[String]): Unit = {
    val shapes: List[Shape] = List(new Circle(1.0), new Square(2.0))
    shapes.foreach(s => println(s))
    println(shapes.map(_.area).sum)
    val b = new Both
    println(b.tag + " " + b.name + " " + b.area)
    println(b.isInstanceOf[Shape])
    println(b.isInstanceOf[Tagged])
  }
}
