// A lower bound is checked too: `B` must be a supertype of `A`.
sealed trait Shape
case class Circle(r: Int) extends Shape

class Box[A](val value: A) {
  def widen[B >: A](other: B): Box[B] = new Box[B](other)
}

object Main {
  def main(args: Array[String]): Unit = {
    val b = new Box[Shape](Circle(1))
    println(b.widen[Circle](Circle(2)).value)
  }
}
