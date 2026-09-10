import scala.language.experimental.macros
import scala.reflect.macros.blackbox.Context
class ConstructorVal(val x: Int)
class ConstructorVar(var x: Int)
class BodyVal { val x: Int = 1 }
class BodyVar { var x: Int = 1 }
class ImplicitField { implicit final val x: Int = 1 }
class StringField(val x: String)
trait AbstractField { val x: Int }
object FieldMacros {
  def noInt[T]: String = macro FieldMacrosImpl.noInt[T]
}
object FieldMacrosImpl {
  def noInt[T: c.WeakTypeTag](c: Context) = {
    import c.universe._
    val tpe = weakTypeOf[T]
    if (tpe.members.exists(_.info =:= typeOf[Int]))
      c.abort(c.enclosingPosition, "integer storage is forbidden")
    c.Expr[String](Literal(Constant("ok")))
  }
}
