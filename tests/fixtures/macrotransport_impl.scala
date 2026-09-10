import scala.reflect.macros.blackbox.Context
object MacroTransportImpl {
  def constant(c: Context): c.Expr[Int] = {
    import c.universe._
    c.Expr[Int](Literal(Constant(42)))
  }
  def empty(c: Context)(): c.Expr[Int] = constant(c)
  def finalEmpty(c: Context)(a: c.Expr[Int])(): c.Expr[Int] = a
  def typeShapes(c: Context): c.Expr[Int] = {
    import c.universe._
    assert(c.typecheck(q"(x: Int) => x + 2").tpe =:= typeOf[Int => Int])
    assert(c.typecheck(q"(40, 2)").tpe =:= typeOf[(Int, Int)])
    assert(c.typecheck(q"Array(40, 2)").tpe =:= typeOf[Array[Int]])
    constant(c)
  }
  def block(c: Context)(a: c.Expr[Int]): c.Expr[Int] =
    c.Expr[Int](c.untypecheck(a.tree))
  def silent(c: Context): c.Expr[Int] = {
    import c.universe._
    val tree = c.typecheck(Select(Literal(Constant(10)), TermName("$minus")), silent = true)
    assert(tree == EmptyTree)
    c.Expr[Int](Literal(Constant(42)))
  }
  object Marker
  def attached(c: Context): c.Expr[Int] = {
    import c.universe._
    val tree = internal.updateAttachment(Ident(TermName("bar")), Marker)
    assert(internal.attachments(tree).get[Marker.type].isDefined)
    val typed = c.typecheck(tree)
    assert(internal.attachments(typed).get[Marker.type].isDefined)
    c.Expr[Int](typed)
  }
  def repeated(c: Context)(args: c.Expr[Int]*): c.Expr[Int] = {
    import c.universe._
    c.Expr[Int](args.foldLeft[Tree](Literal(Constant(0)))((sum, arg) => q"$sum + $arg"))
  }
  def position(c: Context): c.Expr[String] = {
    import c.universe._
    val pos = c.macroApplication.pos
    c.Expr[String](Literal(Constant(pos.source.lineToString(pos.line - 1).substring(pos.column))))
  }
  def local(c: Context): c.Expr[Int] = {
    import c.universe._
    val tree = q"{ class Local { def answer: Int = 42 }; new Local().answer }"
    c.Expr[Int](c.untypecheck(c.typecheck(tree)))
  }
}
