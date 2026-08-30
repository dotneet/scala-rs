// The argument paths that do not go through the ordinary call: a `this(…)`
// constructor delegation, a trait's `$init$`, default and named arguments, a
// second parameter list, and a by-name `Unit`. Plus two `Unit` parameters in
// a row (each takes a slot of its own), a `try`/`catch` and a `match` whose
// result is `Unit`, and a recursive call through a lifted local `def`.
class F(val a: Unit, val b: Int) {
  def this(b: Int) = this((), b)
}

trait Init {
  val iv: Unit = ()
  def useIv(u: Unit): String = "iv"
}

class G extends Init

object Main {
  def d(u: Unit = (), n: Int = 1): String = "d" + n
  def named(n: Int, u: Unit): String = "n" + n
  def curried(u: Unit)(n: Int): String = "c" + n
  def byname(u: => Unit): String = { u; "by" }
  def two(a: Unit, b: Unit, c: Int): Int = c
  def tryIt(u: Unit): Unit = try { u } catch { case _: Throwable => () }
  def matchIt(x: Int): Unit = x match {
    case 0 => ()
    case _ => ()
  }

  def main(args: Array[String]): Unit = {
    println(two((), (), 9))
    println(tryIt(()))
    println(matchIt(0))
    def rec(u: Unit, n: Int): Int = if (n == 0) 0 else rec((), n - 1)
    println(rec((), 3))
    println(new F(7).b)
    println(new F(7).a)
    println(new G().iv)
    println(new G().useIv(()))
    println(d())
    println(d(n = 3))
    println(d((), 4))
    println(named(u = (), n = 5))
    println(curried(())(6))
    println(byname(()))
  }
}
