import scala.language.experimental.macros
import scala.reflect.macros.whitebox.Context
trait MissingEvidence
trait Evidence[A] { type Out; def value: Out }
object Evidence {
  type Aux[A, R] = Evidence[A] { type Out = R }
  implicit def materialize[A, R]: Aux[A, R] = macro implementation[A, R]
  implicit def unavailable[A, R](implicit missing: MissingEvidence): Aux[A, R] = macro unavailableImpl[A, R]
  def unavailableImpl[A: c.WeakTypeTag, R: c.WeakTypeTag](c: Context)(missing: c.Expr[MissingEvidence]): c.Tree = {
    println("unavailable-output-fit-expanded")
    c.abort(c.enclosingPosition, "candidate without required evidence must not expand")
  }
  def implementation[A: c.WeakTypeTag, R: c.WeakTypeTag](c: Context): c.Tree = {
    import c.universe._
    println("output-fit-expanded")
    val enclosing = c.openImplicits.map(_.tree.symbol.name.toString)
    if (enclosing.contains("derive")) assert(enclosing.count(_ == "derive") == 1)
    val input = weakTypeOf[A]
    q"new Evidence[$input] { type Out = String; def value: String = ${input.toString} }"
  }
}
trait Render[A] { def apply(value: A): String }
object Render {
  implicit val string: Render[String] = new Render[String] { def apply(value: String): String = value + "!" }
}
trait Result[A] { def value: String }
object Result {
  implicit def derive[A, R](implicit evidence: Evidence.Aux[A, R], render: Render[R]): Result[A] =
    new Result[A] { def value: String = render(evidence.value) }
}
