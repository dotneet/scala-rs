object Main {
  def show(e: Int Either String): Int = if (e.isLeft) 1 else 2
  def main(args: Array[String]): Unit = {
    val l: Int Either String = Left(7)
    val r: Int Either String = Right("ok")
    println(show(l))
    println(show(r))
  }
}
