trait T {
  val x: Int
  val y: Int = x + 10
}
class Early extends { val x = 1 } with T
class Late extends T {
  val x = 1
}
object Main {
  def main(args: Array[String]): Unit = {
    println(new Early().y)
    println(new Late().y)
  }
}
