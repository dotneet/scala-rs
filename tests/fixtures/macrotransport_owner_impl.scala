import scala.reflect.macros.blackbox.Context
object OwnerImpl {
  def wrap(c: Context)(a: c.Expr[Int]): c.Expr[Int] = {
    import c.universe._
    val owner = c.internal.enclosingOwner
    assert(owner.isTerm && !owner.isMethod && !owner.isClass)
    assert(owner.fullName == "Main.field" || owner.fullName == "Main.local" || owner.fullName == "Main.definitions")
    val g = c.typecheck(q"(x: Int) => $a")
    val fn = g.asInstanceOf[Function]
    assert(g.symbol != NoSymbol && g.symbol.owner == owner)
    assert(g.symbol.name.toString == "$anonfun" && g.symbol.info == NoType)
    assert(fn.vparams.head.symbol.owner == g.symbol)
    assert(fn.vparams.head.symbol.info =:= typeOf[Int])
    val stats = a.tree.asInstanceOf[Block].stats
    assert(stats.nonEmpty && stats.forall(_.symbol.owner == owner))
    c.universe.internal.changeOwner(a.tree, owner, g.symbol)
    assert(stats.forall(_.symbol.owner == g.symbol))
    c.internal.changeOwner(a.tree, g.symbol, owner)
    assert(stats.forall(_.symbol.owner == owner))
    c.universe.internal.changeOwner(a.tree, owner, g.symbol)
    assert(stats.forall(_.symbol.owner == g.symbol))
    // A second typecheck must preserve the function's identity.
    assert(c.typecheck(g).symbol == g.symbol)
    println("owner 日本語 " + owner.name)
    System.err.println("owner stderr 日本語 " + owner.name)
    c.Expr[Int](q"$g(2)")
  }
  def recursive(c: Context)(a: c.Expr[Int]): c.Expr[Int] = {
    val forced = c.internal.enclosingOwner.info
    a
  }
}
