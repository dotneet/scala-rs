// SAM conversion to Scala traits and abstract classes: primitive and
// generic signatures, SAMs with concrete members that must keep working,
// a SAM lambda capturing vars, and SAM-typed overloads.
object Main {
  trait IntOp { def apply(x: Int): Int; def twice(x: Int): Int = apply(apply(x)) }
  trait Pred[A] { def test(a: A): Boolean; def negate: Pred[A] = a => !test(a) }
  abstract class Handler { var count = 0; def handle(s: String): String; def run(s: String): String = { count += 1; handle(s) } }
  trait Combine[A] { def combine(a: A, b: A): A }
  trait Thunk { def get(): String }
  trait DoubleFn { def f(d: Double): Double }
  trait ToUnit { def act(i: Int): Unit }
  def useOp(op: IntOp): Int = op.twice(5)
  def reduceWith[A](xs: List[A], c: Combine[A]): A = xs.reduce(c.combine)
  def main(args: Array[String]): Unit = {
    val inc: IntOp = x => x + 1
    println(inc(1) + " " + inc.twice(1) + " " + useOp(_ * 3))
    val even: Pred[Int] = _ % 2 == 0
    println(even.test(4) + " " + even.negate.test(4) + " " + List(1, 2, 3, 4).filter(even.test))
    val h: Handler = s => s.reverse
    println(h.run("abc") + " " + h.run("xy") + " count=" + h.count)
    println(reduceWith[Int](List(1, 2, 3), _ + _) + " " + reduceWith[String](List("a", "b"), (x, y) => y + x))
    var captured = 0
    val th: Thunk = () => { captured += 1; "thunk" + captured }
    println(th.get() + th.get() + " " + captured)
    val df: DoubleFn = math.sqrt(_)
    println(df.f(2.25))
    var sink = 0
    val tu: ToUnit = i => sink += i
    tu.act(5); tu.act(6)
    println(sink)
    val ops: List[IntOp] = List[IntOp](_ + 10, x => x * x, (x: Int) => -x)
    println(ops.map(_(3)) + " " + ops.map(_.twice(2)))
    val fromMethod: IntOp = Math.abs(_: Int)
    println(fromMethod(-9))
  }
}
