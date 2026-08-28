object Main {
  def main(args: Array[String]): Unit = {
    val t = (1, "b", true)
    println(t._1)
    println(t._2)
    println(t._3)
    val (x, y, z) = t
    println(x)
    println(z)
    t match { case (a, _, c) => println(a + ":" + c) }
  }
}
