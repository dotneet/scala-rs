// An eta-expansion evaluates its receiver once, when the function value is
// built (nsc `etaExpand` lifts an impure prefix into a `val`). scala-rs used
// to leave the receiver inside the lambda: `mk().m` re-ran `mk()` per call.
// A stable prefix -- a `val`, a lazy `val`, `this`, a module -- is not
// lifted, so a lazy receiver is still forced at the first call.
class K(n: Int) {
  def m(a: Int): Int = a + n
  def p[A](a: A): List[A] = List.fill(n)(a)
  def c(a: Int)(b: Int): Int = a + b + n
}
class W(val k: K) {
  var cnt = 0
  def mk(): K = { cnt += 1; new K(cnt) }
  def fs: List[Int => Int] = List(mk().m, mk().m _)
  def g: String => List[String] = mk().p[String]
}
object Main {
  var calls = 0
  def mk(): K = { calls += 1; println("mk " + calls); new K(calls) }
  lazy val lk: K = { println("lazy"); new K(100) }
  def main(args: Array[String]): Unit = {
    val f: Int => Int = mk().m
    println(f(1) + f(2))
    val w = new W(new K(1))
    val fs = w.fs
    println(fs.map(_(10)))
    println(fs.map(_(10)))
    val g = w.g
    println(g("a")); println(g("b"))
    val h: Int => Int = w.k.m
    println(h(1))
    val l: Int => Int = lk.m
    println("after l")
    println(l(1))
    val xs = List(1, 2, 3).map(mk().m)
    println(xs + " calls=" + calls)
    val cur: Int => Int => Int = mk().c
    println(cur(1)(2) + cur(3)(4) + " calls=" + calls)
    val u = mk().m _
    println(u(1) + u(2) + " calls=" + calls)
  }
}
