object Main {
  def main(args: Array[String]): Unit = {
    println(Lib.greet("Scala"))
    println(Lib.greet("Scala", "?"))
    println(Lib.magic)
    println(Lib.id(42))
    println(new Box("hi").get)
    println(Lib.add(Point(3, 4)))
    println(new Point(1, 2).x)
    println(Lib.one)
    println(Lib.lit(1))
  }
}
