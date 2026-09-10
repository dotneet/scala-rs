package typeidentitymacro
import scala.reflect.macros.blackbox.Context
object Impl {
  def value(c: Context): c.Expr[Int] = { import c.universe._; c.Expr[Int](q"42") }
  def sum(c: Context)(x: c.Expr[Int])(y: c.Expr[Int]): c.Expr[Int] = {
    import c.universe._; c.Expr[Int](q"$x + $y")
  }
  def names[A, B](c: Context)(implicit b: c.WeakTypeTag[B], a: c.WeakTypeTag[A]): c.Expr[String] = {
    import c.universe._
    val text = weakTypeOf[A].toString + ":" + weakTypeOf[B].toString
    c.Expr[String](Literal(Constant(text)))
  }
  def choose[A: c.WeakTypeTag](c: Context): c.Tree = {
    import c.universe._
    val t = weakTypeOf[A]
    if (t =:= typeOf[Int]) c.abort(c.enclosingPosition, "Int evidence is forbidden")
    q"new _root_.typeidentitymacro.EvidenceValue[$t](77)"
  }
}
