import scala.language.experimental.macros
import scala.reflect.macros.blackbox
object RefinedBundle {
  def identity[A](a: A): A = macro RefinedBundleImpl.identity[A]
  def answer: Int = macro RefinedBundleImpl.answer
  def rejected: Int = macro RejectBundle.answer
}
class RejectBundle(val c: blackbox.Context) {
  c.abort(c.enclosingPosition, "bundle constructor rejected")
  def answer: c.Expr[Int] = c.Expr[Int](c.universe.Literal(c.universe.Constant(0)))
}
class RefinedBundleImpl(val c: blackbox.Context) {
  def identity[A: c.WeakTypeTag](a: c.Expr[A]): c.Expr[A] = a
  def answer: c.Expr[Int] = {
    import c.universe._
    c.Expr[Int](Literal(Constant(42)))
  }
}
