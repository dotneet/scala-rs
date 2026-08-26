object Main {
  def main(args: Array[String]): Unit = {
    println(Lib.greet("Scala"))
    println(Lib.greet("Scala", "?"))
    println(Lib.magic)
    println(Lib.id(42))
    println(new Box("hi").get)
  }
}
