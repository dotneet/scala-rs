import scala.language.experimental.macros
import scala.reflect.macros.blackbox

object ReflectFanout {
  def many: Int = macro ReflectFanoutImpl.many
  def one: Int = macro ReflectFanoutImpl.one
}

object ReflectFanoutImpl {
  def many(c: blackbox.Context): c.Expr[Int] = {
    import c.universe._
    val statements = (0 until 384).map { index =>
      val name = TermName("value" + index)
      q"val $name = ReflectFanout.one"
    }
    c.Expr[Int](q"{ ..$statements; 1 }")
  }

  def one(c: blackbox.Context): c.Expr[Int] = {
    import c.universe._
    val required = List(typeOf[Ordering[Int]], typeOf[Numeric[Int]], typeOf[Ordering[String]])
    required.foreach { tpe =>
      if (c.inferImplicitValue(tpe) == EmptyTree)
        c.abort(c.enclosingPosition, "missing " + tpe)
    }
    val typed = c.typecheck(q"scala.Predef.identity[Int](1)")
    if (!(typed.tpe <:< typeOf[Int]))
      c.abort(c.enclosingPosition, "unexpected typed result")
    c.Expr[Int](q"1")
  }
}
