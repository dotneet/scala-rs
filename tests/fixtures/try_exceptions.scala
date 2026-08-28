object Main {
  def main(args: Array[String]): Unit = {
    try {
      throw new IllegalArgumentException("bad")
    } catch {
      case e: IllegalArgumentException => println(e.getMessage)
    }
    try {
      println(1 / 0)
    } catch {
      case e: ArithmeticException => println("arith: " + e.getMessage)
    }
    try {
      throw new RuntimeException("boom")
    } catch {
      case e: RuntimeException => println(e.getMessage)
    }
  }
}
