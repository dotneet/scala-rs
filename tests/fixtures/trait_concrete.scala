trait T {
  def greet(): String = "from trait"
}
class C extends T
object Main {
  def main(args: Array[String]): Unit = {
    println(new C().greet())
  }
}
