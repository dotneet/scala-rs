// nsc `convertToAssignment`'s `mkUpdate` branch: when the receiver of an
// op-assignment is an indexing, `t(i) op= x` is `t.update(i, t.apply(i) op x)`,
// with the table and every index evaluated once (`gen.evalOnce`). Without it
// `arr(0) += 1` was reported as an unassignable receiver.
final class Counter {
  private val cells = new Array[Int](4)
  var reads = 0
  def apply(i: Int): Int = { reads += 1; cells(i) }
  def update(i: Int, v: Int): Unit = cells(i) = v
}

object Main {
  var calls = 0
  val shared = new Array[Int](2)
  def table(): Array[Int] = { calls += 1; shared }
  def index(): Int = { calls += 1; 0 }

  def main(args: Array[String]): Unit = {
    val arr = new Array[Int](2)
    arr(0) = 1
    arr(0) += 5
    println(arr(0))
    arr(0) -= 2
    arr(0) *= 3
    println(arr(0))

    // Compound right-hand side, which needs the op-assignment precedence too.
    val i = 1
    val x = 2
    arr(1) = 0
    arr(1) += i + x
    println(arr(1))

    val d = new Array[Double](1)
    d(0) = 1.5
    d(0) += 0.5
    println(d(0))

    val s = new Array[String](1)
    s(0) = "a"
    s(0) += "b"
    println(s(0))

    // Nested indexing: the inner `t(i)` is the table.
    val nested = new Array[Array[Int]](2)
    nested(0) = new Array[Int](2)
    nested(0)(1) = 4
    nested(0)(1) += 3
    println(nested(0)(1))

    // A user-defined `apply` / `update` pair, not an array.
    val c = new Counter
    c(2) = 10
    c(2) += 5
    println(c(2))

    // `gen.evalOnce`: the table and the index run once, not twice.
    shared(0) = 0
    table()(index()) += 1
    println(shared(0))
    println(calls)
  }
}
