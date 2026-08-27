object Main {
  def main(args: Array[String]): Unit = {
    val ys = Array(1, 2, 3).map(_ + 1)
    ys.foreach(x => println(x))
    val f: Int => Int = _ + 1
    println(f(10))
    val g: Int => Int = _.abs
    println(g(-3))
    def add1(x: Int): Int = x + 1
    val h: Int => Int = add1(_)
    println(h(4))
  }
}
