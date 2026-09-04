// The rejection side of agent/arraygen. Real scalac 2.13.16 rejects these three too.
// Confirms that building descriptors from the declaration did not also start
// letting ill-typed `Array` operations through.
import scala.collection.immutable.HashSet
object Main {
  // An explicit type argument has to line up with the element type.
  def bad(s: HashSet[String]): HashSet[Int] = s.map[Int](x => x)
  // A `String` does not go into an element of `Array[Int]`.
  def worse(a: Array[Int]): Unit = a(0) = "x"
  // `Array` is not free to pick the element type of the varargs either.
  def worst(a: Array[Int]): String = render(a: _*)
  def render(parts: String*): String = parts.mkString
  def main(args: Array[String]): Unit = ()
}
