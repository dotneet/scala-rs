// The shapes scalac 2.13.16 still rejects, so that giving the prelude the
// library's flags and parameter names does not turn into accepting anything.

import scala.util.Success

object Main {
  // `case class Some[+A](value: A)` -- the field is `value`, not `x`.
  val a = Some(1).copy(x = 2)
  // `copy` takes exactly the class's own fields.
  val b = Success(1).copy(value = 2, extra = 3)
  // `mkString`'s parameters are `start`, `sep`, `end`.
  val c = List(1, 2).mkString(separator = "-")
  // `map` takes one parameter, called `f`.
  val d = List(1, 2).map(g = (x: Int) => x)
  // `::` takes one type parameter, not two.
  val e: ::[Int, String] = new ::(1, Nil)

  def main(args: Array[String]): Unit = ()
}
