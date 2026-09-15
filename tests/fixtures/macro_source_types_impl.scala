import scala.reflect.macros.blackbox.Context

object SourceTypesImpl {
  def inspect[A](c: Context)(implicit tag: c.WeakTypeTag[A]): c.Tree = {
    import c.universe._
    val t = tag.tpe
    val params = t.typeSymbol.asClass.typeParams.map { p =>
      p.name.toString + ":" + p.typeSignature.toString + ":" + p.asType.isCovariant
    }
    Literal(Constant(params.mkString(";") + "|" + t.member(TermName("value")).typeSignatureIn(t).toString))
  }
  def shape[A](c: Context)(implicit tag: c.WeakTypeTag[A]): c.Tree = {
    import c.universe._
    val t = tag.tpe
    Literal(Constant(t.toString + "|" + t.typeArgs.mkString(",")))
  }
  def conforms[A, B](c: Context)(implicit a: c.WeakTypeTag[A], b: c.WeakTypeTag[B]): c.Tree = {
    import c.universe._
    Literal(Constant(a.tpe <:< b.tpe))
  }
  def echo[A](c: Context)(value: c.Expr[A])(implicit tag: c.WeakTypeTag[A]): c.Tree = {
    import c.universe._
    Typed(value.tree, TypeTree(tag.tpe))
  }
}
