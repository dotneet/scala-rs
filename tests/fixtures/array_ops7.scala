object Main {
  def main(args: Array[String]): Unit = {
    println(Array(1.0, 2.0).head)
    val ys = Array(1.0, 2.0).map(_ + 1.0)
    ys.foreach(x => println(x))
    println(Array(true, false).head)
    val zs = Array(true, false).map(!_)
    zs.foreach(x => println(x))
  }
}
