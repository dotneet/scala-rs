object Main {
  def main(args: Array[String]): Unit = {
    println("hello".dropWhile(_ != 'l'))
    println("hello".takeWhile(_ != 'l'))
    println("hi".nonEmpty)
    println("".nonEmpty)
    println("ab".headOption)
    println("".headOption)
    println("ab".lastOption)
    println("hello".filterNot(_ == 'l'))
  }
}
