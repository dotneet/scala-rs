object Main {
  def main(args: Array[String]): Unit = {
    println('a' to 'c')
    println('a' until 'c')
    println(('a' to 'c').mkString(","))
    println(('a' until 'c').mkString(","))
  }
}
