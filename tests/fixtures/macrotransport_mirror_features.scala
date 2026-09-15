package mirrorprobe
import scala.language.experimental.macros
import scala.reflect.macros.blackbox.Context
object MirrorFeatures { def inspect[T]: String = macro MirrorFeaturesMacros.inspect[T] }
object MirrorFeaturesMacros {
 def inspect[T:c.WeakTypeTag](c:Context):c.Expr[String]={
  import c.universe._
  val t = weakTypeOf[T]
  val alias = t.decl(TypeName("Alias"))
  val nested = t.decl(TypeName("Nested"))
  val value = t.decl(TermName("value"))
  val obj = t.decl(TermName("Marker"))
  val text = alias.isType.toString + ":" + alias.info.toString + ":" + nested.isClass + ":" + nested.owner.name.toString + ":" + value.privateWithin.name.toString + ":" + obj.isModule
  c.Expr[String](Literal(Constant(text)))
 }
}
