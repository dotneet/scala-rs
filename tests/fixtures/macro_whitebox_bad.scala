// Whitebox macros are not implemented; slick needs only blackbox ones.
// See docs/macros.md §6.3.
import scala.reflect.macros.whitebox.Context

object Macros {
  def implF(c: Context)(): Int = 0
}

object Sugar {
  def f(): Int = macro Macros.implF
}

object Main {
  def main(args: Array[String]): Unit = println("unreachable")
}
