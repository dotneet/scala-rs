// Non-local return from closures: foreach, nested lambdas, return through a
// try/catch that catches Throwable-subclass handlers, and from a by-name body.
object Main {
  def findFirst(xs: List[Int], p: Int => Boolean): Option[Int] = {
    xs.foreach { x => if (p(x)) return Some(x) }
    None
  }
  def nested(xss: List[List[Int]]): Int = {
    xss.foreach(xs => xs.foreach(x => if (x > 10) return x))
    -1
  }
  def throughCatch(xs: List[Int]): String = {
    try {
      xs.foreach(x => if (x == 2) return "found two")
      "not found"
    } catch {
      case e: RuntimeException => "caught runtime " + e
    }
  }
  def throughFinally(xs: List[Int]): String = {
    val sb = new StringBuilder
    try { xs.foreach(x => { sb.append(x); if (x == 3) return "early:" + sb }) ; "end:" + sb } finally { sb.append("!") }
  }
  def byName(body: => Unit): Unit = body
  def fromByName(): String = { byName { return "from by-name" }; "after" }
  def inMap(xs: List[Int]): List[Int] = xs.map(x => if (x < 0) return Nil else x * 2)
  def loopReturn(): Int = { var i = 0; while (true) { i += 1; if (i == 5) return i }; 0 }
  def optionReturn(o: Option[Int]): Int = { o.foreach(v => return v * 100); 0 }
  def nestedDefs(xs: List[Int]): Int = {
    def inner(): Int = { xs.foreach(x => if (x % 7 == 0) return x); 0 }
    inner() + 1
  }
  def unitReturn(xs: List[Int]): Unit = { xs.foreach(x => if (x == 1) { println("unit return at 1"); return }); println("no 1") }
  def longReturn(xs: List[Long]): Long = { xs.foreach(x => if (x > 5L) return x); 0L }
  def doubleReturn(xs: List[Double]): Double = { xs.foreach(x => if (x > 0.5) return x); -1.0 }

  def main(args: Array[String]): Unit = {
    println(findFirst(List(1, 3, 6, 8), _ % 2 == 0)); println(findFirst(List(1, 3), _ > 5))
    println(nested(List(List(1, 2), List(3, 20, 30)))); println(nested(List(List(1))))
    println(throughCatch(List(1, 2, 3))); println(throughCatch(Nil))
    println(throughFinally(List(1, 2, 3, 4))); println(throughFinally(List(1)))
    println(fromByName()); println(inMap(List(1, 2))); println(inMap(List(1, -2, 3)))
    println(loopReturn()); println(optionReturn(Some(3))); println(optionReturn(None))
    println(nestedDefs(List(1, 14, 21))); println(nestedDefs(List(1)))
    unitReturn(List(0, 1, 2)); unitReturn(List(5))
    println(longReturn(List(1L, 9L))); println(doubleReturn(List(0.1, 0.9)))
  }
}
