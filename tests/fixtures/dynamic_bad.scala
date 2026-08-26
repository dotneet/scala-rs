class D extends Dynamic {
  def selectDynamic(name: String): String = name
}
object Main {
  def main(args: Array[String]): Unit = {
    val d = new D()
    println(d.foo)
  }
}
