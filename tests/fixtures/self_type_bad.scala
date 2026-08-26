trait Foo {
  def n: Int = 10
}
trait Add { self: Foo =>
  def plus(x: Int): Int = x + n
}
class Bad extends Add
object Main {
  def main(args: Array[String]): Unit = ()
}
