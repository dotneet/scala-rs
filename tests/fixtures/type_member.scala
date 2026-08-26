trait Foo {
  type A
  def x: A
}
class Bar extends Foo {
  type A = Int
  def x: A = 41
}
object Main {
  def fromProj(n: Bar#A): Int = n + 1
  def main(args: Array[String]): Unit = {
    val n: Int = new Bar().x
    println(n)
    println(fromProj(n))
  }
}
