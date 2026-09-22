import scala.language.experimental.macros
import scala.reflect.macros.whitebox.Context

trait ValueCodec[A] { def name: String }
final case class WrappedValue[A](value: ValueCodec[A])
object ReceiverMacro {
  def wrapped[A]: WrappedValue[A] = macro implementation[A]
  def implementation[A: c.WeakTypeTag](c: Context): c.Expr[WrappedValue[A]] = {
    import c.universe._
    val tpe = weakTypeOf[A]
    c.Expr[WrappedValue[A]](q"new WrappedValue[$tpe](new ValueCodec[$tpe] { def name: String = ${tpe.toString} })")
  }
}
