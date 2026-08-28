// Calling a macro must be diagnosed, not silently accepted: the macro def has
// no bytecode, so a silent pass would emit a call to a method that is not there.
import scala.reflect.macros.blackbox.Context

object Macros {
  def implG(c: Context)(x: Int): Int = 0
}

object Sugar {
  def g(x: Int): Int = macro Macros.implG
}

object Main {
  def main(args: Array[String]): Unit = {
    println(Sugar.g(1))
  }
}
