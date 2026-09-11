// scala-rs rejects: a name-based extractor whose `get` result has _1/_2
// (product-like), matched with two sub-patterns.
object Main {
  final class PairRes(a: Int, b: Int) { def isEmpty = false; def get = this; def _1 = a; def _2 = b }
  object DivMod { def unapply(n: Int): PairRes = new PairRes(n / 3, n % 3) }
  def main(args: Array[String]): Unit = {
    val DivMod(q, r) = 17
    println(s"$q $r")
    20 match { case DivMod(a, b) => println(a * 10 + b) }
  }
}
