import scala.util.control.NonFatal
object Main {
  class MyEx(m: String) extends RuntimeException(m)
  def risky(n: Int): Int = if (n < 0) throw new MyEx("neg") else n * 2
  def main(a: Array[String]): Unit = {
    try { println(risky(-1)) } catch { case e: MyEx => println("caught " + e.getMessage) }
    try { println(risky(3)) } catch { case NonFatal(e) => println("nf") } finally { println("fin") }
    val r = try risky(-2) catch { case _: Throwable => -99 }
    println(r)
    println(scala.util.Try(risky(-1)).recover { case e: MyEx => 0 }.get)
    def loop(n: Int): Int = try { if (n == 0) 1 else n * loop(n - 1) } finally {}
    println(loop(5))
  }
}
