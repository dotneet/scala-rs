// Lenient prototypes, negatives: the outer call still rejects a builder
// whose element conflicts with the other argument, an explicit element,
// and a collection that is not the declared one. With a *covariant*
// collection (`List`) the prototype's `List[String]` gives `newBuilder`
// only an upper bound, and nsc minimises `A` to `Nothing` -- so even the
// "matching" shapes are rejected, exactly as scalac rejects them.
import scala.collection.mutable
object Main {
  def fill[B, C2](b: mutable.Builder[B, C2], x: B): C2 = { b += x; b.result() }
  def m[B, C2](b: mutable.Builder[B, C2], f: Int => B): C2 = { b += f(1); b.result() }
  val g: Int => String = _.toString
  val a: List[Int] = fill(List.newBuilder, "s")          // line 12
  val b: List[String] = fill(List.newBuilder[Int], "s")  // line 13
  val c: Vector[Int] = fill(List.newBuilder, 1)          // line 14
  val d: List[String] = m(List.newBuilder, g)            // line 15
  val e = fill(Vector.newBuilder, 'c')                   // line 16
  def main(args: Array[String]): Unit = ()
}
