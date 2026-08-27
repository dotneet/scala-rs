object Main {
  @native def foo(): Int = 1
  def main(args: Array[String]): Unit = {
    println(foo())
  }
}
