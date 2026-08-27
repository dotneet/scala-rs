object Main {
  def main(args: Array[String]): Unit = {
    Array(1, 2, 3).drop(1).foreach(x => println(x))
    Array(1, 2, 3, 4).dropWhile(_ < 3).foreach(x => println(x))
    println(Array(1, 2, 3).exists(_ == 2))
    println(Array(1, 2, 3).exists(_ == 9))
  }
}
