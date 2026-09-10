// Valid Scala covering both implemented transport and the remaining singleton
// type-tag gap. Block/function cases execute in macrotransportbatch; this file
// retains all cases and must still diagnose the unrepresentable singleton tag.
import scala.language.experimental.macros

object EgGaps {
  def plus1(x: Int): Int = macro EgImpl.plusImpl
  def nameOf[T]: String = macro EgImpl.nameOfImpl[T]
}

object Main {
  def main(args: Array[String]): Unit = {
    // Supported block argument, executed separately in macrotransportbatch.
    println(EgGaps.plus1({ val a = 1; a }))
    // Supported function literals, through a call and a direct application.
    println(EgGaps.plus1(List(1).map((n: Int) => n).head))
    println(EgGaps.plus1(((n: Int) => n)(1)))
    // A type argument no `staticClass` call can rebuild.
    println(EgGaps.nameOf[Main.type])
  }
}
