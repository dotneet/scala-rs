import scala.annotation.switch

object Main {
  def dense(n: Int): Int = (n: @switch) match {
    case 0 => 10
    case 1 => 11
    case 2 => 12
    case 3 => 13
    case 4 => 14
  }
  def sparse(n: Int): Int = (n: @switch) match {
    case 0 => 1
    case 100 => 2
    case 200 => 3
  }
  def main(args: Array[String]): Unit = {
    println(dense(0))
    println(dense(4))
    println(dense(2))
    println(sparse(100))
  }
}
