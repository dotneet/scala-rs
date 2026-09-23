case class SameRunDefaultRow(number: Int = 7, text: String = "ok")

object Main {
  def main(args: Array[String]): Unit =
    println(ConstructorDefaultMacro.make())
}
