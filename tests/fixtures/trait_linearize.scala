trait A {
  def msg: String = "A"
}
trait B {
  def msg: String = "B"
}
class C extends A with B
object Main {
  def main(args: Array[String]): Unit = {
    println(new C().msg)
  }
}
