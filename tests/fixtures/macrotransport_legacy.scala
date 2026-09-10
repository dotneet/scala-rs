import scala.language.experimental.macros
object EgGaps { def plus1(x: Int): Int = macro EgImpl.plusImpl }
class ExGap(val tag: String) { def label: String = macro ExImpl.tagImpl }
object Main {
  var constructed = 0
  def main(args: Array[String]): Unit = {
    println(EgGaps.plus1({ val a = 1; a }))
    println(EgGaps.plus1(List(1).map((n: Int) => n).head))
    println(EgGaps.plus1(((n: Int) => n)(1)))
    println(new ExGap({ constructed += 1; "x" }).label)
    println(constructed)
  }
}
