import scala.language.experimental.macros
import scala.reflect.macros.blackbox.Context
object MacroArgs {
 def types(xs: Int*): String = macro ArgImpl.types
}
object ArgImpl {
 def types(c: Context)(xs: c.Expr[Int]*): c.Expr[String] = {
  import c.universe._
  c.Expr[String](Literal(Constant(xs.map(x => x.tree.tpe.toString).mkString("|"))))
 }
}
