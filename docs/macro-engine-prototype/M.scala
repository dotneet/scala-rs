import scala.reflect.macros.blackbox.Context
import scala.language.experimental.macros

object M {
  def impl(c: Context)(): c.Expr[Int] = {
    import c.universe._
    c.Expr[Int](Literal(Constant(42)))
  }
  def f(): Int = macro impl

  def plusImpl(c: Context)(x: c.Expr[Int]): c.Expr[Int] = {
    import c.universe._
    c.Expr[Int](Apply(Select(x.tree, TermName("$plus")), List(Literal(Constant(1)))))
  }
  def plus1(x: Int): Int = macro plusImpl
}
