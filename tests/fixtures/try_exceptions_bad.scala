object Main {
  def main(args: Array[String]): Unit = {
    val e = new ArithmeticException("x")
    println(e.noSuchThrowableMember)
  }
}
