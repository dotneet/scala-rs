object Main {
  def main(args: Array[String]): Unit = {
    val s = Set(1, 2, 3)
    println(s.contains(2))
    println(s.contains(0))
    s.foreach(x => println(x))
  }
}
