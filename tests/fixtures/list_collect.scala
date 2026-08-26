object Main {
  def main(args: Array[String]): Unit = {
    val pf: PartialFunction[Int, Int] = { case 1 => 10; case 2 => 20 }
    val ys = List(1, 2, 3).collect(pf)
    ys.foreach((x: Int) => println(x))
  }
}
