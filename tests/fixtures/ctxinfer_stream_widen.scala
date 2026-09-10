object Main {
  def widen(xs: Stream[String]): Stream[AnyRef] = xs
  def main(args: Array[String]): Unit = {
    val a: Stream[AnyRef] = widen(Stream("a", "b"))
    println(a.toList.mkString(":"))
  }
}
