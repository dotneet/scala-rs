class A {
  def tag: String = "A"
}
class B extends A {
  override def tag: String = "B"
}
class Parent {
  implicit val more: B = new B()
}
object Main extends Parent {
  implicit val less: A = new A()
  def pick(implicit x: A): String = x.tag
  def main(args: Array[String]): Unit = {
    println(pick())
  }
}
