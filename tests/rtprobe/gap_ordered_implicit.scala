// scala-rs rejects: Ordering[A] for A <: Ordered[A] (Ordering.ordered via
// AsComparable), the Ordering.BigInt instance, and seqOrdering with
// explicit type arguments.
object Main {
  case class Ver(major: Int, minor: Int) extends Ordered[Ver] {
    def compare(o: Ver): Int = if (major != o.major) major - o.major else minor - o.minor
  }
  def main(args: Array[String]): Unit = {
    val vs = List(Ver(1, 2), Ver(0, 9), Ver(1, 0))
    println(vs.sorted + " " + vs.max)
    println(List(BigInt(5), BigInt(-2)).sorted)
    println(List(List(2, 1), List(1, 9, 9), List(1, 9)).sorted(Ordering.Implicits.seqOrdering[List, Int]))
  }
}
