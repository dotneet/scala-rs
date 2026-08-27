object Main {
  type A = B
  type B = A
  def main(args: Array[String]): Unit = {
    val x: A = 1
  }
}
