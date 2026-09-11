// lazy val: evaluated at most once on success, re-evaluated after a throwing
// initializer, local lazy vals, lazy vals in traits, and forward references.
object Main {
  var count = 0
  lazy val once = { count += 1; "once" + count }

  var attempts = 0
  lazy val flaky: Int = { attempts += 1; if (attempts < 3) throw new RuntimeException("try " + attempts); attempts * 10 }

  trait LT { def base: Int; lazy val derived = { println("computing derived"); base * 2 } }
  class LC(val base: Int) extends LT

  class Forward { lazy val a: Int = b + 1; lazy val b: Int = 41 }

  def local(): String = {
    var evals = 0
    lazy val x = { evals += 1; evals }
    val before = evals
    val s = x + x + x
    s"before=$before sum=$s evals=$evals"
  }

  def neverForced(): String = {
    lazy val boom: Int = throw new IllegalStateException("forced")
    "not forced"
  }

  class Cell { lazy val self: Cell = this }

  def main(args: Array[String]): Unit = {
    println(count)
    println(once); println(once); println(count)
    for (_ <- 1 to 4) {
      try println("flaky=" + flaky)
      catch { case e: RuntimeException => println("caught " + e.getMessage) }
    }
    val lc = new LC(4)
    println("created")
    println(lc.derived); println(lc.derived)
    println(new Forward().a)
    println(local())
    println(neverForced())
    val c = new Cell
    println(c.self eq c)
  }
}
