// The errors the sixth slice must keep reporting.

object Main {
  def fu(g: Int => Unit): Unit = g(1)
  val h: Int => Int = (x: Int) => x + 1
  // A function *value* is not adapted to a different result type; only a
  // literal's body is checked against the expected one. nsc rejects this.
  def bad1(): Unit = fu(h)

  // `map` on a `Map` is the key lookup, so the key type still has to match.
  def bad2(m: Map[String, Int]): IndexedSeq[Int] = IndexedSeq(1, 2).map(m)

  // An `ArrayBuffer` is an `IndexedSeq`, but not a `Vector`.
  def bad4(b: scala.collection.mutable.ArrayBuffer[Int]): Vector[Int] = b

  // A `Failure` still has to be a `Try` of the right element type.
  def bad5(e: Throwable): scala.util.Try[Int] = scala.util.Success("x")

  def main(args: Array[String]): Unit = println(bad1())
}
