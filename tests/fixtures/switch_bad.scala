import scala.annotation.switch

object Main {
  def f(n: Any): Int = (n: @switch) match {
    case 0 => 1
    case "x" => 2
    case _ => 3
  }
  def main(args: Array[String]): Unit = {
    println(f(0))
  }
}
