object Main {
  def main(args: Array[String]): Unit = {
    Array(1, 2, 3).filter(_ > 1).foreach(x => println(x))
    Array(1, 2, 3, 4).slice(1, 3).foreach(x => println(x))
    Array(1, 2).flatMap(x => List(x, x + 10)).foreach(x => println(x))
  }
}
