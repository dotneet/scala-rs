// Collection factories whose element types only meet at a lub: the pairs
// `(Int, Int)` and `(Int, Double)` make a `Map[Int, AnyVal]` (no weak lub
// inside a type argument), and similar mixed literals for List/Set/Vector.
object Main {
  def main(args: Array[String]): Unit = {
    val m = Map(1 -> 2, 3 -> 4.5)
    println(m)
    println(m(3))
    val l = List(1 -> "a", 2 -> 'b')
    println(l)
    println(Set(1, "x", 2.0).size)
    println(Vector(Some(1), None, Some("s")))
    println(List(1, 2.5, 3L))
    println(Map("a" -> List(1), "b" -> Vector(2)).values.map(_.sum))
  }
}
