object Main {
  def main(args: Array[String]): Unit = {
    val xs = Seq(1, 2, 3)
    xs.foreach(x => println(x))
    val ys = LazyList(1, 2, 3)
    ys.foreach(x => println(x))
  }
}
