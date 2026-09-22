import scala.language.experimental.macros
import scala.reflect.macros.whitebox.Context
trait ConstructorBox[F[_]]
object ConstructorAliases { type Identity[A] = A }
object ConstructorMacro {
  def inspect[A]: String = macro implementation[A]
  def implementation[A: c.WeakTypeTag](c: Context): c.Expr[String] = {
    import c.universe._
    c.typecheck(q"null.asInstanceOf[${weakTypeOf[A]}]")
    val constructor = weakTypeOf[A].typeArgs.head
    val applied = appliedType(constructor, typeOf[String]).dealias
    c.Expr[String](Literal(Constant(applied.toString)))
  }
}
