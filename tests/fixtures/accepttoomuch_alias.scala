// `agent/accepttoomuch`: the shapes the new rejection rules must NOT reject.
//
// 1. A local `type` alias. A block ran the namer over its `class` and `object`
//    statements only, so `type Row = List[Int]` had no symbol and every
//    signature naming it kept an unresolved placeholder. That was invisible
//    while such a placeholder was tolerated; it is a `not found: type Row`
//    now, so the alias has to be entered for real. cats' `Monad.ifElseM` is
//    the shape: an alias followed by a nested `def` whose parameter names it.
//
// 2. Two overloads that erasure separates by their *result* type alone.
//    `scala.Function.uncurried` is five of them; real scalac 2.13.16 accepts
//    this and rejects the same pair with equal results ("double definition").
object Main {
  def size(xs: List[Int]): Int = {
    type Row = List[Int]
    def go(ys: Row): Int = ys.length
    go(xs)
  }

  def pair(n: Int): String = {
    type Pair = (Int, String)
    val p: Pair = (n, "x")
    p._1.toString + p._2
  }

  def widen(xs: List[Int]): Int = xs.length
  def widen(xs: List[String]): String = xs.mkString(",")

  def main(args: Array[String]): Unit = {
    println(size(List(1, 2, 3)))
    println(pair(7))
    println(widen(List(1, 2)))
    println(widen(List("a", "b")))
  }
}
