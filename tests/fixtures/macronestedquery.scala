import scala.language.experimental.macros
import scala.reflect.macros.whitebox.Context
final case class NestedEvidence[A](name: String)
object NestedEvidence {
  implicit def make[A]: NestedEvidence[A] = macro implementation[A]
  def implementation[A: c.WeakTypeTag](c: Context): c.Tree = {
    import c.universe._
    assert(c.openMacros.head eq c)
    assert(c.openMacros.length >= 3)
    q"new NestedEvidence[${weakTypeOf[A]}](${weakTypeOf[A].toString})"
  }
}
object NestedQuery {
  def select[A](value: A): String = macro implementation[A]
  def refined[A]: Int = macro refinedImplementation[A]
  def implementation[A: c.WeakTypeTag](c: Context)(value: c.Expr[A]): c.Expr[String] = {
    import c.universe._
    val before = c.openMacros
    val evidence = c.inferImplicitValue(appliedType(typeOf[NestedEvidence[_]].typeConstructor, weakTypeOf[A]))
    assert(c.openMacros == before)
    val checked = c.typecheck(q"$evidence.name")
    c.Expr[String](q"{ $value; $checked }")
  }
  def refinedImplementation[A: c.WeakTypeTag](c: Context): c.Expr[Int] = {
    import c.universe._
    val refined = c.internal.intersectionType(List(weakTypeOf[A], typeOf[Product]))
    val wanted = appliedType(typeOf[NestedEvidence[_]].typeConstructor, refined)
    val evidence = c.inferImplicitValue(wanted)
    assert(evidence != EmptyTree)
    c.Expr[Int](q"{ val nested = $evidence; 42 }")
  }
}
