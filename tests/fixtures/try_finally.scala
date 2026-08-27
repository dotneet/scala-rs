object Main {
  def main(args: Array[String]): Unit = {
    try {
      println("ok")
    } finally {
      println("fin-ok")
    }
    try {
      try {
        println("before-throw")
        throw new RuntimeException()
      } finally {
        println("fin-throw")
      }
    } catch {
      case _: RuntimeException => println("outer")
    }
    try {
      try {
        throw new RuntimeException()
      } catch {
        case _: RuntimeException =>
          println("caught")
          throw new RuntimeException()
      } finally {
        println("fin-catch")
      }
    } catch {
      case _: RuntimeException => println("outer2")
    }
  }
}
