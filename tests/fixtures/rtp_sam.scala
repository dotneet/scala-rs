// SAM conversion: a SAM whose method takes primitives receives them in
// primitive slots (a `long` or `double` in two), and a SAM that is an
// abstract class is extended, not implemented.
object Main {
  trait IntOp { def apply(x: Int): Int; def twice(x: Int): Int = apply(apply(x)) }
  trait DOp { def f(d: Double): Double }
  trait Mixed { def m(a: Long, b: String, c: Double, d: Boolean): String }
  abstract class Handler { var count = 0; def handle(s: String): String; def run(s: String): String = { count += 1; handle(s) } }
  abstract class Scorer { def score(n: Int): Long }
  def main(args: Array[String]): Unit = {
    val ops: List[IntOp] = List(_ + 10, x => x * x)
    println(ops.map(_(3)) + " " + ops.map(_.twice(2)))
    val d: DOp = x => x / 2
    println(d.f(3.0))
    val m: Mixed = (a, b, c, flag) => s"$a $b $c $flag"
    println(m.m(1L << 40, "s", 2.5, true))
    val h: Handler = s => s.reverse
    println(h.run("abc") + " " + h.run("xy") + " count=" + h.count)
    val sc: Scorer = n => n.toLong * 1000000000L
    println(sc.score(7))
    println(java.util.stream.IntStream.rangeClosed(1, 3).map(x => x * x).sum())
    val cmp: java.util.Comparator[Integer] = (a, b) => b.compareTo(a)
    val al = new java.util.ArrayList[Integer](); al.add(1); al.add(3); al.add(2); al.sort(cmp)
    println(al)
  }
}
