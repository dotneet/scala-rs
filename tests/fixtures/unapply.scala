object Even {
  def unapply(n: Int): Option[Int] = if (n % 2 == 0) Some(n / 2) else None
}
object Main {
  def main(args: Array[String]): Unit = {
    val x = 10 match {
      case Even(half) => half
      case _ => 0
    }
    println(x)
    val y = 7 match {
      case Even(half) => half
      case _ => -1
    }
    println(y)
  }
}
