object Main {
  def main(args: Array[String]): Unit = {
    val xs = Array(1, 2, 3)
    xs.foreach(x => println(x))
    val ys = Array(10L, 20L, 30L)
    println(ys.head)
    ys.foreach(x => println(x))
  }
}
