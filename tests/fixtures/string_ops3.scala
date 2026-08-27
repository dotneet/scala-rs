object Main {
  def main(args: Array[String]): Unit = {
    println("foobar".stripSuffix("bar"))
    println("ab".padTo(5, 'x'))
    println("a\nb".linesIterator.next())
    println("12".toIntOption)
    println("x".toIntOption)
  }
}
