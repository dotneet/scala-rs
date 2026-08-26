object Main {
  def id[T](x: T): T = x
  def main(args: Array[String]): Unit = {
    println(id(42))
    println(id("hi"))
  }
}
