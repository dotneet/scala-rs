object Main {
  def main(args: Array[String]): Unit = {
    val a = Array(1, 2, 3)
    println(a.last)
    a.init.foreach(x => println(x))
    a.reverse.foreach(x => println(x))
    println(a.size)
    println(a.isEmpty)
    println(a.nonEmpty)
  }
}
