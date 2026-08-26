trait A { implicit val x: Int = 1 }
trait B { implicit val y: Int = 2 }
object Main extends A with B {
  def add(n: Int)(implicit z: Int): Int = n + z
  def main(args: Array[String]): Unit = {
    println(add(0))
  }
}
