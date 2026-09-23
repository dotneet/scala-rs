import binaryextractor.Word

object Main {
  def main(args: Array[String]): Unit = {
    "xyz" match {
      case Word(value) => println(value)
      case _ => println("miss")
    }
    "abc" match {
      case Word(value) => println(value)
      case _ => println("miss")
    }
  }
}
