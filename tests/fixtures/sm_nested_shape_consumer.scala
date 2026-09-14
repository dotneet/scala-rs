import smnestedshape._
import smnestedshape.Profile.api._

object Main {
  val a = new Rep[String]("a")
  val b = new Rep[Int]("b")
  val c = new Rep[Long]("c")
  val triple: ShapedValue[(Rep[String], Rep[Int], Rep[Long]), (String, Int, Long)] = (a, b, c)
  // The outer tuple3Shape recursively derives the first element's tuple3Shape.
  // Both packed types are left open by ShapedValue's expected type.
  val nested: ShapedValue[
    ((Rep[String], Rep[Int], Rep[Long]), Rep[String], Rep[Int]),
    ((String, Int, Long), String, Int)
  ] = ((a, b, c), a, b)

  def main(args: Array[String]): Unit = println(triple.describe)
}
