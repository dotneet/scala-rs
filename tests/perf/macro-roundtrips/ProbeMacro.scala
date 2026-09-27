import scala.language.experimental.macros
import scala.reflect.macros.blackbox

object Probe {
  def repeat(count: Int): Int = macro ProbeMacro.repeat
}

object ProbeMacro {
  def repeat(c: blackbox.Context)(count: c.Expr[Int]): c.Expr[Int] = {
    import c.universe._
    val Literal(Constant(n: Int)) = count.tree: @unchecked
    val first = c.parse("1 + 2")
    val second = c.parse("1 + 2")
    if (first.asInstanceOf[AnyRef] eq second.asInstanceOf[AnyRef])
      c.abort(c.enclosingPosition, "c.parse reused a tree instance")
    var i = 0
    while (i < n) {
      c.typecheck(c.parse("1 + 2"))
      i += 1
    }
    c.Expr[Int](Literal(Constant(42)))
  }
}
