trait A { def a: Int }
trait B { def b: Int }
class C extends A with B {
  def a: Int = 1
  def b: Int = 2
}
object Main {
  def use(x: A with B): Int = x.a + x.b
  def main(args: Array[String]): Unit = {
    val v: A with B = new C()
    println(use(v))
    println(use(new C()))
  }
}
