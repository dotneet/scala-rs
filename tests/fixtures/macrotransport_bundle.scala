import scala.language.experimental.macros
import scala.reflect.macros.whitebox

trait LiteralEvidence[T] { def value: T }
object LiteralEvidence {
  implicit def materialize[T]: LiteralEvidence[T] = macro LiteralBundle.materialize[T]
}
class LiteralBundle(val c: whitebox.Context) {
  def materialize[T: c.WeakTypeTag]: c.Expr[LiteralEvidence[T]] = {
    import c.universe._
    val tpe = weakTypeOf[T]
    val value = tpe match {
      case ConstantType(value) => Literal(value)
      case _ => c.abort(c.enclosingPosition, "type mismatch: expected a literal type")
    }
    c.Expr[LiteralEvidence[T]](q"new LiteralEvidence[$tpe] { def value: $tpe = $value }")
  }
}
