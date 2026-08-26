trait Greeter {
  def greet(name: String): String
}

class HelloGreeter extends Greeter {
  def greet(name: String): String = "Hello, " + name
}

object Main {
  def main(args: Array[String]): Unit = {
    val g: Greeter = new HelloGreeter()
    println(g.greet("Scala"))
  }
}
