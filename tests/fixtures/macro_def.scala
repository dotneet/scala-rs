// A def macro is parsed and recorded. `M.f` is never called here, so nothing
// has to be expanded and the file compiles. `Macros` is never loaded at run
// time either — the macro def itself gets no bytecode.
import scala.reflect.macros.blackbox.Context

object Macros {
  def implF(c: Context)(): Int = 0
  def implG(c: Context)(x: Int): Int = x
}

object Sugar {
  def f(): Int = macro Macros.implF
  def g(x: Int): Int = macro Macros.implG
}

object Main {
  def main(args: Array[String]): Unit = {
    println("macro def compiled")
  }
}
