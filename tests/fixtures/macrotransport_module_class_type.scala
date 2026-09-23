import scala.language.experimental.macros
import scala.reflect.macros.blackbox.Context

object StaticChoice

object ModuleClassTypeMacro {
  def materialize(): String = macro materializeImpl

  def materializeImpl(c: Context)(): c.Expr[String] = {
    import c.universe._
    val moduleClassType = weakTypeOf[StaticChoice.type].typeSymbol.asClass.toType
    c.Expr[String](q"""{ val ignored = null.asInstanceOf[$moduleClassType]; "ok" }""")
  }
}
