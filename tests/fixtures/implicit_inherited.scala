class Base {
  implicit val n: Int = 10
}
object Main extends Base {
  def add(x: Int)(implicit y: Int): Int = x + y
  def main(args: Array[String]): Unit = {
    println(add(5))
  }
}
