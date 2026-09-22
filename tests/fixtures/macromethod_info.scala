import scala.language.experimental.macros
import scala.reflect.macros.blackbox
object MethodInfo {
  def inferred[A]:Int = macro implementation[A]
  def implementation[A:c.WeakTypeTag](c:blackbox.Context):c.Expr[Int] = {
    import c.universe._
    val member = weakTypeOf[A].decl(TermName("hidden"))
    if (!(member.typeSignature.finalResultType =:= typeOf[Int]))
      c.abort(c.enclosingPosition, "expected inferred Int result")
    c.Expr[Int](Literal(Constant(42)))
  }
}
