object Main {
  def main(args: Array[String]): Unit = {
    println(Array('a', 'b').head)
    val ys = Array('a', 'b').map(_ + 1)
    ys.foreach(x => println(x))
    println(Array(1.0f, 2.0f).head)
    val zs = Array(-1.0f, 2.0f).map(_.abs)
    zs.foreach(x => println(x))
  }
}
