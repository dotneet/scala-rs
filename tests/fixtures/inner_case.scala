// A case class's companion module and a value class both need their own
// `InnerClasses` self-entry too.
object Main {
  case class Point(x: Int, y: Int)
  class ValueBox(val n: Int) extends AnyVal

  def main(args: Array[String]): Unit = {
    val p = Point(1, 2)
    println(p.getClass.isMemberClass)
    println(p.getClass.getSimpleName)
    println(Point.getClass.isMemberClass)
    println(classOf[ValueBox].isMemberClass)
    println(classOf[ValueBox].getSimpleName)
  }
}
