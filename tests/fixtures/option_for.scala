object Main {
  def main(args: Array[String]): Unit = {
    val a = Some(3)
    val b = for (x <- a) yield x + 1
    println(b.get)
    val n: Option[Int] = None
    val c = for (x <- n) yield x + 1
    println(c.isEmpty)
  }
}
