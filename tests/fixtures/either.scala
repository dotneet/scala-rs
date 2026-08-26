object Main {
  def main(args: Array[String]): Unit = {
    val r = Right(1)
    println(r.isLeft)
    println(r.getOrElse(0))
    val l = Left("err")
    println(l.isLeft)
    println(l.getOrElse(0))
  }
}
