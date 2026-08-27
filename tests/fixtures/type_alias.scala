trait M {
  type A = String
  def id(x: A): A = x
}
class C extends M
object Main {
  type T = List[Int]
  def headOf(xs: T): Int = xs.head
  def main(args: Array[String]): Unit = {
    val xs: T = 1 :: 2 :: Nil
    println(headOf(xs))
    println(new C().id("ok"))
  }
}
