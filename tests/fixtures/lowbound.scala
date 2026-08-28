sealed trait Shape
case class Circle(r: Int) extends Shape
case class Rect(w: Int, h: Int) extends Shape

/// A user-declared `[B >: A]`: `widen` must join the argument type with `A`.
class Box[A](val value: A) {
  def widen[B >: A](other: B): Box[B] = new Box[B](other)
}

object Main {
  def area(s: Shape): Int = s match {
    case Circle(r)  => r * r * 3
    case Rect(w, h) => w * h
  }

  // `[A <: Shape]` lets `x` be used where a `Shape` is wanted.
  def twice[A <: Shape](x: A): Int = area(x) * 2

  def main(args: Array[String]): Unit = {
    // `::` is `[B >: A](elem: B): List[B]`, so this is a `List[Shape]`,
    // not a `List[Circle]`.
    val shapes = Circle(1) :: Rect(2, 3) :: Nil
    shapes.foreach(s => println(area(s)))

    // Existing monomorphic uses keep their precise element type.
    val ints = 1 :: 2 :: 3 :: Nil
    ints.foreach(i => println(i + 1))
    val strs = "a" :: Nil
    strs.foreach(s => println(s + "!"))
    println(Nil.isEmpty)

    // Lower bound on a user-defined class.
    val boxed = new Box[Circle](Circle(4))
    val widened = boxed.widen(Rect(5, 6))
    println(area(widened.value))

    // Upper bound, inferred and explicit.
    println(twice(Rect(2, 3)))
    println(twice[Circle](Circle(2)))
  }
}
