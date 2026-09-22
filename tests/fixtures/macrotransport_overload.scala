import scala.language.experimental.macros
import scala.reflect.macros.blackbox.Context

object ChosenMacro {
  def inspect(a: String, b: Throwable): String = macro ChosenMacroImpl.cause
  def inspect(a: String, b: Int): String = macro ChosenMacroImpl.number
}
object ChosenMacroImpl {
  def cause(c: Context)(a: c.Expr[String], b: c.Expr[Throwable]): c.Expr[String] = {
    import c.universe._
    c.Expr[String](Literal(Constant(c.macroApplication.symbol.asMethod.paramLists.head(1).typeSignature.toString)))
  }
  def number(c: Context)(a: c.Expr[String], b: c.Expr[Int]): c.Expr[String] = {
    import c.universe._
    c.Expr[String](Literal(Constant(c.macroApplication.symbol.asMethod.paramLists.head(1).typeSignature.toString)))
  }
}
