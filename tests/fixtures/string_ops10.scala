object Main {
  def main(args: Array[String]): Unit = {
    println("hello".filter(c => c != 'l'))
    "hello".reverseIterator.foreach(c => println(c))
  }
}
