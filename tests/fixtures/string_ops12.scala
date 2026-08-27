object Main {
  def main(args: Array[String]): Unit = {
    println("hello".updated(1, 'a'))
    println("hello".count(_ == 'l'))
    println("hello".span(_ != 'l'))
  }
}
