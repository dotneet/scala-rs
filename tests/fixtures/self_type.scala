trait Foo {
  def n: Int = 10
}
trait Add { self: Foo =>
  def plus(x: Int): Int = x + n
}
class C extends Foo with Add
object Main {
  def main(args: Array[String]): Unit = {
    println(new C().plus(5))
  }
}
