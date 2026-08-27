object Main {
  def main(args: Array[String]): Unit = {
    println(Array(1.toByte, 2.toByte).head)
    val ys = Array(1.toByte, 2.toByte).map(_ + 1)
    ys.foreach(x => println(x))
    println(Array(1.toShort, 2.toShort).head)
    val zs = Array(1.toShort, 2.toShort).map(_ + 1)
    zs.foreach(x => println(x))
  }
}
