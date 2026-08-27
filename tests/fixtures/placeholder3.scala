object Main {
  def main(args: Array[String]): Unit = {
    val f: Int => Int = (_: Int) + 1
    println(f(10))
    val g = (_: Int) + (_: Int)
    println(g(1, 2))
    val h: Int => Int = (_: Int).abs
    println(h(-3))
    Array(1, 2, 3).map((_: Int) + 1).foreach(x => println(x))
    val nest: Array[Int] => Array[Int] = _.map((_: Int) + 1)
    nest(Array(1, 2, 3)).foreach(x => println(x))
  }
}
