trait Greeter {
  def greet(name: String): String
}

object Main {
  def main(args: Array[String]): Unit = {
    val g: Greeter = new Greeter { def greet(name: String): String = "Hello, " + name }
    println(g.greet("Scala"))
    val a = new { def msg: String = "anon" }
    println(a.msg)
  }
}
