class A {
  def tag: String = "A"
}
class B extends A {
  def tag: String = "B"
}
object Main {
  implicit val a: A = new A()
  implicit val b: B = new B()
  def pick(implicit x: A): String = x.tag
  def main(args: Array[String]): Unit = {
    println(pick())
  }
}
