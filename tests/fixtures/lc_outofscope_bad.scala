// An implicit class local to one method is not visible in a sibling method.
object Main {
  def other(): Unit = {
    implicit class F(val n: Int) { def dbl: Int = n * 2 }
  }
  def main(a: Array[String]): Unit = {
    println(3.dbl)
  }
}
