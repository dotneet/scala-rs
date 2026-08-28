// nsc: "unknown parameter name: q" — for a method and for a constructor.
class C(val a: Int, val b: Int)
object Main {
  def f(a: Int, b: Int): Int = a + b
  def main(args: Array[String]): Unit = {
    println(f(q = 1, b = 2))
    println(new C(q = 1, b = 2).a)
  }
}
