import macrofixture.MacroTypes
package object macroaliases { val W = MacroTypes }
object Main {
  type One = macroaliases.W.`1`.T
  def main(args: Array[String]): Unit = {
    val one: One = 1
    val result: Int = MacroTypes.expression("val x = 40; x + 2")
    println(one)
    println(result)
  }
}
