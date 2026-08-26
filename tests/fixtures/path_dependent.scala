trait Foo {
  type A
  def x: A
}
class Bar extends Foo {
  type A = Int
  def x: A = 41
}
object Main {
  def fromPath(c: Foo { type A = Int }): c.A = c.x
  def main(args: Array[String]): Unit = {
    println(fromPath(new Bar()))
    println(fromPath(new Bar()) + 1)
  }
}
