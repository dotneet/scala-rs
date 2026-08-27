object Main {
  def main(args: Array[String]): Unit = {
    println("hello".head)
    println("hello".last)
    println("hello\n".stripLineEnd)
    println("a-b-a".replaceAllLiterally("-", "_"))
  }
}
