class A {
  def tag: String = "A"
}
class B extends A {
  override def tag: String = "B"
}
class Parent {
  implicit val less: A = new A()
}
object Main extends Parent {
  implicit val more: B = new B()
  def pick(implicit x: A): String = x.tag
  def main(args: Array[String]): Unit = {
    println(pick())
  }
}
