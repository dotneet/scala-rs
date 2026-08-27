object Main {
  def main(args: Array[String]): Unit = {
    println("hello".find(_ == 'l'))
    println("hello".find(_ == 'z'))
    "hi".foreach(c => println(c))
    println("true".toBoolean)
    println("false".toBoolean)
    println("true".toBooleanOption)
    println("nope".toBooleanOption)
  }
}
