import scala.language.experimental.macros
import scala.reflect.macros.whitebox.Context
trait IntersectionMarker[A]
object IntersectionMacro {
  def value: Any = macro implementation
  def implementation(c: Context): c.Tree = {
    import c.universe._
    val label = c.internal.constantType(Constant("label"))
    val marker = appliedType(typeOf[IntersectionMarker[_]].typeConstructor, label)
    val result = c.internal.refinedType(List(typeOf[java.io.Serializable], marker), NoSymbol)
    q"null.asInstanceOf[$result]"
  }
}
