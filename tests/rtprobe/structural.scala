// Structural types through reflective calls: methods with primitive and
// reference parameters and results, and duck-typed close().
import scala.language.reflectiveCalls
object Main {
  def closeIt(c: { def close(): Unit }): Unit = c.close()
  def area(s: { def w: Int; def h: Int }): Int = s.w * s.h
  def greet(g: { def greet(name: String): String }): String = g.greet("struct")
  def calc(c: { def twice(x: Int): Int; def half(d: Double): Double }): String = c.twice(4) + " " + c.half(3.0)
  class Res { var closed = false; def close(): Unit = closed = true }
  class Rect(val w: Int, val h: Int)
  def main(args: Array[String]): Unit = {
    val r = new Res
    closeIt(r)
    println(r.closed)
    println(area(new Rect(3, 4)))
    println(area(new { val w = 2; val h = 5 }))
    println(greet(new { def greet(name: String) = "hi " + name }))
    println(calc(new { def twice(x: Int) = x * 2; def half(d: Double) = d / 2 }))
    val anon = new { def value = 42; def plus(o: Int) = value + o }
    println(anon.value + " " + anon.plus(1))
  }
}
