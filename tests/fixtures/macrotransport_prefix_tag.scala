import scala.language.experimental.macros
import scala.reflect.macros.blackbox.Context

class TaggedOwner[T](val value: T)(implicit val tag: scala.reflect.runtime.universe.WeakTypeTag[T]) {
  def choose[D](value: D): D = macro TaggedOwnerMacros.choose[D]
}
object TaggedOwnerMacros {
  def choose[D: c.WeakTypeTag](c: Context)(value: c.Expr[D]): c.Expr[D] = {
    import c.universe._
    assert(c.settings.isEmpty)
    val prefix = c.prefix.tree
    assert(prefix.tpe != null)
    assert(prefix.tpe.typeSymbol.name.toString == "TaggedOwner")
    assert(c.prefix.staticType =:= typeOf[Nothing])
    c.Expr[D](q"""{
      import scala.collection.immutable.{List => DataList, _}
      val owner = $prefix
      println(owner.tag.tpe)
      if (owner == null) throw new IllegalStateException("missing owner")
      DataList($value).head
    }""")
  }
}
