import scala.language.experimental.macros
import scala.reflect.macros.blackbox

object Fanout {
  def many: Int = macro FanoutImpl.many
  def one: Int = macro FanoutImpl.one
}

object FanoutImpl {
  def many(c: blackbox.Context): c.Expr[Int] = {
    import c.universe._
    val statements = (0 until 256).map { index =>
      val name = TermName("value" + index)
      q"val $name = Fanout.one"
    }
    c.Expr[Int](q"{ ..$statements; 1 }")
  }

  def one(c: blackbox.Context): c.Expr[Int] = {
    import c.universe._
    if (c.inferImplicitValue(typeOf[Ordering[Int]]) == EmptyTree)
      c.abort(c.enclosingPosition, "missing Ordering[Int]")
    c.Expr[Int](q"1")
  }
}
