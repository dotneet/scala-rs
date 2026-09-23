import scala.language.experimental.macros
import scala.reflect.macros.blackbox.Context

class WildBox[A]

object WildcardTagMacro {
  def inspect[T]: String = macro inspectImpl[T]

  def inspectImpl[T: c.WeakTypeTag](c: Context): c.Expr[String] = {
    import c.universe._
    val tpe = weakTypeOf[T]
    assert(tpe.typeSymbol.name.toString == "WildBox")
    assert(tpe.typeArgs.size == 1)
    assert(tpe.typeArgs.head != null)
    val wildcard = tpe.typeArgs.head
    val result = s"${tpe.typeSymbol.name}:${tpe.typeArgs.size}"
    c.Expr[String](q"""{
      val ignored = null.asInstanceOf[WildBox[$wildcard]]
      $result
    }""")
  }
}
