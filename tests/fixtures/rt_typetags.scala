import scala.reflect.runtime.universe._

object Main {
  def main(args: Array[String]): Unit = {
    println(implicitly[TypeTag[Int]].tpe.toString)
    println(implicitly[WeakTypeTag[String]].tpe.toString)
    val t: Transformer = null
    println(t == null)
    val rm = runtimeMirror(this.getClass.getClassLoader)
    println(rm.getClass.getName.nonEmpty)
  }
}
