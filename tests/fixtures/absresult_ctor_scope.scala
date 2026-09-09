object Outer {
  implicit val evidence: Int = 7
  val f: Int = 9
  class C(val x: Int) {
    def f: Int = 100
    def this() = { this(f); println(f) }
    def this(s: String) = this(implicitly[Int] + Outer.this.f)
  }
}
class Param(val x: Int) {
  def this(s: String)(implicit n: Int) = this(implicitly[Int])
}
object Main {
  def main(args: Array[String]): Unit = {
    println(new Outer.C().x)
    println(new Outer.C("x").x)
    println(new Param("x")(11).x)
  }
}
