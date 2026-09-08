// An under-applied type-argument list was accepted. Scala 2 has no partial
// application of a type constructor, so `new Cell[K]` for a two-parameter
// `Cell` is an error; `apply_types` reported only the *over*-applied
// direction, and the under-applied one silently kept the arguments it had and
// filled the rest with the class's own parameters.
//
// scalac 2.13.16: "wrong number of type arguments for ovsc.Cell, should be 2"
// on line 15. This compiler prints the class's simple name, which is what
// `display_type` gives everywhere else.
package ovsc

class Cell[K, V](val k: K, val v: V)

object CountBad {
  def main(args: Array[String]): Unit = {
    val b = new Cell[String]("a", 1)
    println(b.k)
  }
}
