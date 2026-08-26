import scala.language.dynamics
class D extends Dynamic {
  def selectDynamic(name: String): String = name
  def applyDynamic(name: String)(x: String): String = name + x
}
object Main {
  def main(args: Array[String]): Unit = {
    val d = new D()
    println(d.foo)
    println(d.bar("x"))
  }
}
