import scala.language.experimental.macros
import scala.reflect.macros.whitebox.Context
trait Associated[A] { type Out; def value: Out }
object Associated {
  type Aux[A, R] = Associated[A] { type Out = R }
  implicit def materialize[A, R]: Aux[A, R] = macro implementation[A, R]
  def implementation[A: c.WeakTypeTag, R: c.WeakTypeTag](c: Context): c.Tree = {
    import c.universe._
    val input = weakTypeOf[A]
    q"new Associated[$input] { type Out = String; def value: String = ${input.toString} }"
  }
}
