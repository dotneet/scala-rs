object Main {
  def main(args: Array[String]): Unit = {
    println("|hello\n|world".stripMargin)
    println("#hello\n#world".stripMargin('#'))
    println("a\nb".lines.next())
  }
}
