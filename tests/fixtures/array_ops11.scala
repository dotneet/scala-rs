object Main {
  def main(args: Array[String]): Unit = {
    Array(1, 2).flatMap(i => List(i, i)).foreach(x => println(x))
    Array(1, 2).flatMap(i => Array(i, i)).foreach(x => println(x))
  }
}
