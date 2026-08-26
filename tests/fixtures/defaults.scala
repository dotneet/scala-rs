object Main {
  def greet(name: String, punct: String = "!"): String = "hi " + name + punct
  def main(args: Array[String]): Unit = {
    println(greet("Scala"))
    println(greet("Scala", "?"))
  }
}
