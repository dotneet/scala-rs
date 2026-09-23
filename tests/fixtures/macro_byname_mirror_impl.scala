import scala.language.experimental.macros
import scala.reflect.macros.whitebox

object ByNameMirror {
  def inspect[T]: String = macro ByNameMirrorImpl.inspect[T]
}

class ByNameMirrorImpl(val c: whitebox.Context) {
  import c.universe._

  def inspect[T: c.WeakTypeTag]: c.Expr[String] = {
    val method = weakTypeOf[T].decl(TermName("run")).asMethod
    val parameter = method.paramLists.head.head
    val result = if (parameter.typeSignature.toString.startsWith("=>")) "by-name" else "strict"
    c.Expr[String](Literal(Constant(result)))
  }
}
