import scala.language.experimental.macros
import scala.reflect.macros.blackbox
object M { def text: String = macro Impl.text }
object Impl {
  def text(c: blackbox.Context): c.Expr[String] = {
    import c.universe._
    c.Expr[String](Literal(Constant("macro")))
  }
}
