object Main {
  def main(args: Array[String]): Unit = {
    try {
      println("before")
      throw new RuntimeException()
      println("after")
    } catch {
      case _: RuntimeException => println("caught")
    } finally {
      println("finally")
    }
  }
}
