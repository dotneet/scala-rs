class A {
  def tag: String = "A"
}
class B extends A {
  // Real scalac 2.13.16 requires `override` here (SLS 5.1.4); it never got as
  // far as saying so for this file because `pick()` stops it in the typer.
  override def tag: String = "B"
}
object Main {
  implicit val a: A = new A()
  implicit val b: B = new B()
  def pick(implicit x: A): String = x.tag
  def main(args: Array[String]): Unit = {
    println(pick())
  }
}
