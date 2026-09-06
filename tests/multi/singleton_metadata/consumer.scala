class Consumer extends Provider {
  def id[U <: Singleton](value: U): U = value
}
object Main {
  def main(args: Array[String]): Unit = {
    val c = new Consumer
    println(c.id(7))
    println(c.id("bound"))
  }
}
