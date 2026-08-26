object Main {
  def main(args: Array[String]): Unit = {
    println((-3).abs)
    println(1.abs)
    println(1.max(2))
    println(1.to(3))
    1.to(3).foreach(x => println(x))
  }
}
