import scala.language.experimental.macros
import scala.reflect.macros.whitebox
object ReflectionHelpers {
  def inspect[T]: String = macro ReflectionHelperMacros.inspect[T]
}
class ReflectionHelperMacros(context: whitebox.Context) extends shapeless.GenericMacros(context) {
  import c.universe._
  def inspect[T: c.WeakTypeTag]: c.Tree = {
    val tpe = weakTypeOf[T]
    val companion = patchedCompanionSymbolOf(tpe.typeSymbol)
    assert(companion != NoSymbol)
    val ref = companionRef(tpe)
    assert(ref.symbol == companion)
    val publicMethod = tpe.decl(TermName("visible"))
    val privateMethod = tpe.decl(TermName("hidden"))
    assert(isAccessible(tpe, publicMethod))
    assert(!isAccessible(tpe, privateMethod))
    assert(prefix(tpe) == tpe.asInstanceOf[scala.reflect.internal.Types#Type].prefix)
    val selected = mkAttributedRef(prefix(tpe), companion)
    assert(selected.symbol == companion)
    Literal(Constant(companion.name.toString))
  }
}
case class BinaryEntry(value: Int) {
  def visible: Int = value
  private def hidden: Int = value
}
package reflectionops {
  class ++(val value: Int)
  object ++ { def apply(value: Int): ++ = new ++(value) }
}
object SymbolicReflection {
  def value: Int = macro SymbolicReflectionMacros.value
}
class SymbolicReflectionMacros(val c: whitebox.Context) {
  import c.universe._
  def value: c.Tree = q"{ val instance: ${typeOf[reflectionops.++]} = _root_.reflectionops.++(7); instance.value }"
}
