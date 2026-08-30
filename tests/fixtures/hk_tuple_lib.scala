// The reported shape: `println` is overloaded and none of its alternatives
// takes two arguments, so the list is tupled and `println(x: Any)` is the
// one that applies. It is not limited to pairs -- `Tuple3` .. `Tuple22` are
// reached the same way -- and the elements are ordinary expressions, so
// `==`, an extension-method call and a `PartialFunction` member all work
// inside one.
sealed trait Colour
case object Red extends Colour
case class Custom(name: String) extends Colour

object Main {
  val f: PartialFunction[Int, String] = { case 1 => "one" }
  def g(t: (Int, String)): Int = t._1

  def main(args: Array[String]): Unit = {
    println(1, "a")
    println(g(1, "a"))
    println(Red == Red, Red.toString, Custom("a") == Custom("a"))
    println(Predef.identity(3), Predef.implicitly[Int => Int].apply(4))
    println(Set(1, 2) & Set(2, 3), Set(1, 2) | Set(3), Set(1, 2) diff Set(1))
    println(f.isDefinedAt(1), f.applyOrElse(-1, (_: Int) => "neg"))
    println(1, 2, 3, 4)
    println(1, "b", 3.0, true, 'c', 6L)
  }
}
