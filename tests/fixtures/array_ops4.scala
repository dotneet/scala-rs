object Main {
  def main(args: Array[String]): Unit = {
    val ys = Array(1, 2, 3).map(_ + 1)
    ys.foreach(x => println(x))
    val zs = Array("a", "b").map(s => s + "x")
    zs.foreach(s => println(s))
  }
}
