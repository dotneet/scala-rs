object Main {
  def main(args: Array[String]): Unit = {
    println(Array((), ()).head)
    val ys = Array((), ()).map(_ => 1)
    ys.foreach(x => println(x))
  }
}
