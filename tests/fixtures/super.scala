class Base {
  def greet(): String = "base"
}
trait T {
  def greet(): String = "T"
}
class C extends Base {
  def hi(): String = super.greet() + "!"
}
class D extends T {
  def hi(): String = super.greet() + "!"
}
class Outer {
  val name: String = "outer"
  class Inner {
    def who(): String = Outer.this.name
  }
  def inner(): String = new Inner().who()
}
object Main {
  def main(args: Array[String]): Unit = {
    println(new C().hi())
    println(new D().hi())
    println(new Outer().inner())
  }
}
