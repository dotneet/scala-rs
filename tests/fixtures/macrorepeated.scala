import scala.language.experimental.macros
import scala.reflect.macros.blackbox
object RepeatedMirror {
  def count[T]: Int = macro RepeatedMirrorImpl.count[T]
}
object RepeatedMirrorImpl {
  def count[T: c.WeakTypeTag](c: blackbox.Context): c.Expr[Int] = {
    import c.universe._
    val method = weakTypeOf[T].decl(TermName("accept")).asMethod
    val parameter = method.paramLists.head.head.typeSignature
    assert(parameter.typeSymbol == definitions.RepeatedParamClass)
    val element = parameter.typeArgs.head
    assert(element =:= typeOf[Int])
    c.Expr[Int](q"{ def count(values: $parameter): Int = values.size; count(1, 2, 3) }")
  }
}
