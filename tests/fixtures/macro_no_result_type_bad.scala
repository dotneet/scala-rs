// nsc: "macro defs must have explicitly specified return types" — there is
// nothing to check an expansion against otherwise.
import scala.reflect.macros.blackbox.Context

object Macros {
  def implF(c: Context)(): Int = 0
}

object Sugar {
  def f() = macro Macros.implF
}

object Main {
  def main(args: Array[String]): Unit = println("unreachable")
}
