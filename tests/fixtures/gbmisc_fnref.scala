// A method named as the argument of a function- (or SAM-) typed parameter
// whose result is still a type variable: `xs.foreach(println)`. nsc types it
// against `Int => ?` and keeps the alternative that eta-expands to that;
// scala-rs typed it against nothing, auto-applied the nullary `println()`
// and reported `no matching overload ... with arguments (Unit)`.
trait Fun[A, B] { def apply(a: A): B }
class SamInferResult {
  def foreach[U](f: Fun[String, U]): U = f("sam")
  def foo = foreach(println)
}
object Main {
  def show(x: Any): Unit = println("show " + x)
  def ov(): Unit = println("ov0")
  def ov(x: Int): Unit = println("ov1 " + x)
  def ov2(x: String): String = "s" + x
  def ov2(x: Int): Int = x + 1
  def fe[U](f: Int => U): Unit = println(f(1))
  def g(): Int => String = (i: Int) => "g" + i
  val fv: Int => Int = _ + 100
  def fs[U](f: Fun[String, U]): U = f("fs")
  def ite(b: Boolean)(t: String, e: String): String = if (b) t else e
  def main(args: Array[String]): Unit = {
    List(1, 2).foreach(println)
    val r = List(4).map(println)
    println(r)
    List(1, 2) foreach println
    Option(1).foreach(println)
    List(3) foreach show
    List(5, 6).foreach(ov)
    println(List(7).map(ov2))
    println(List("a").map(ov2))
    val f: Int => Unit = println
    f(8)
    fe(println)
    fe(g)
    fe(fv)
    List(9).map(Console.println)
    fe(scala.Predef.println)
    new SamInferResult().foo
    val s: Fun[String, Unit] = println
    s("sam2")
    fs(show)
    println(List(true, false).map(ite).map(h => h("yes", "no")))
  }
}
