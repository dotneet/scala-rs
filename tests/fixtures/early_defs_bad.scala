trait T { val x: Int }
class Bad extends { def f = 1 } with T { val x = 1 }
object Main {
  def main(args: Array[String]): Unit = {
    println(1)
  }
}
