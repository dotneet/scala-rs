import scala.language.dynamics
class D extends Dynamic {
  private var x: String = ""
  def selectDynamic(name: String): String = if (x == "") name else x
  def applyDynamic(name: String)(arg: String): String = name + arg
  def updateDynamic(name: String)(value: String): Unit = { x = value }
  def applyDynamicNamed(name: String)(arg: (String, String)): String = name + arg._1 + arg._2
}
object Main {
  def main(args: Array[String]): Unit = {
    val d = new D()
    println(d.foo)
    println(d.bar("x"))
    d.foo = "ok"
    println(d.foo)
    println(d.baz(a = "y"))
  }
}
