import scala.language.experimental.macros
import scala.reflect.macros.blackbox.Context

trait BinaryOuter { class Inner }
object BinaryCast {
  def cast[T](value: Any): T = macro BinaryCastMacros.cast[T]
}
object BinaryCastMacros {
  def cast[T: c.WeakTypeTag](c: Context)(value: c.Expr[Any]): c.Expr[T] = {
    import c.universe._
    c.Expr[T](q"$value.asInstanceOf[${weakTypeOf[T]}]")
  }
}
