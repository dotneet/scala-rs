import scala.language.experimental.macros
import scala.reflect.macros.blackbox

object ArgumentWork {
  def constant(c: blackbox.Context)(value: c.Expr[Int]): c.Expr[Int] = {
    import c.universe._
    c.Expr[Int](Literal(Constant(1)))
  }
}

object ArgumentProbe {
  def constant(value: Int): Int = macro ArgumentWork.constant
}
