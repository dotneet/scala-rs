package shape

import scala.language.experimental.macros
import scala.reflect.macros.blackbox

object Code {
  def show[A](x: A): String = macro Impl.show[A]
  def rebuild[A](x: A): A = macro Impl.rebuild[A]
  def retyped[A](x: A): A = macro Impl.retyped[A]
}

object Impl {
  def show[A](c: blackbox.Context)(x: c.Expr[A]): c.Expr[String] = {
    import c.universe._
    c.Expr[String](Literal(Constant(showCode(x.tree))))
  }

  // Every application, type application and selection built again from its
  // parts: nothing of the argument goes back as it came.
  def rebuild[A](c: blackbox.Context)(x: c.Expr[A]): c.Expr[A] = {
    import c.universe._
    def copy(t: Tree): Tree = t match {
      case Apply(f, args) => Apply(copy(f), args.map(copy))
      case TypeApply(f, targs) => TypeApply(copy(f), targs.map(a => TypeTree(a.tpe)))
      case Select(q, n) => Select(copy(q), n)
      case other => other.duplicate
    }
    c.Expr[A](copy(x.tree))
  }

  def retyped[A](c: blackbox.Context)(x: c.Expr[A]): c.Expr[A] =
    c.Expr[A](c.untypecheck(x.tree.duplicate))
}
