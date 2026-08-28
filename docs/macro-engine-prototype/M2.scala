import scala.reflect.macros.blackbox.Context
import scala.language.experimental.macros

object Helper { def hello(n: Int): String = "hi " + n }

object M2 {
  // reify, like slick's TableQueryMacroImpl.apply
  def reifyImpl(c: Context): c.Expr[String] = {
    import c.universe._
    c.universe.reify { Helper.hello(7) }
  }
  def r: String = macro reifyImpl

  // weakTypeOf + a WeakTypeTag implicit, like slick's macros
  def tagImpl[T](c: Context)(implicit t: c.WeakTypeTag[T]): c.Expr[String] = {
    import c.universe._
    c.Expr[String](Literal(Constant(t.tpe.toString)))
  }
  def nameOf[T]: String = macro tagImpl[T]

  // quasiquotes, like slick's mapToImpl
  def qqImpl(c: Context)(x: c.Expr[Int]): c.Expr[Int] = {
    import c.universe._
    c.Expr[Int](q"${x.tree} + 1")
  }
  def plus1(x: Int): Int = macro qqImpl
}
