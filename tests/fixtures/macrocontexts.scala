import scala.language.experimental.macros
import scala.reflect.macros.blackbox

object MacroContexts {
  def outer: String = macro MacroContextImpl.outer
  def inner: String = macro MacroContextImpl.inner
}
object MacroContextImpl {
  case class Marker(value: String)
  private var outerContext: blackbox.Context = null

  private def marker(c: blackbox.Context): Option[Marker] =
    c.internal.attachments(c.macroApplication).get[Marker]

  def outer(c: blackbox.Context): c.Expr[String] = {
    import c.universe._
    assert(c.openMacros.size == 2)
    assert(c.openMacros.forall(_ eq c))
    val enclosing = c.enclosingMacros
    assert(enclosing.size == 1)
    assert(enclosing.head eq c)
    c.internal.updateAttachment(c.macroApplication, Marker("outer"))
    assert(marker(c).contains(Marker("outer")))
    outerContext = c
    c.Expr[String](q"MacroContexts.inner")
  }

  def inner(c: blackbox.Context): c.Expr[String] = {
    import c.universe._
    val contexts = c.openMacros
    val enclosing = c.enclosingMacros
    assert(contexts.head eq c)
    assert(contexts.tail.head eq c)
    if (contexts.size == 3) {
      assert(contexts.last eq outerContext)
      assert(contexts.last.macroApplication eq outerContext.macroApplication)
      assert(contexts.last.prefix eq outerContext.prefix)
      assert(contexts.last.openMacros.head eq outerContext)
      assert(contexts.last.openMacros.tail.head eq c)
      assert(marker(contexts.last).contains(Marker("outer")))
      assert(enclosing.size == 2)
      assert(enclosing.head eq c)
      assert(enclosing.tail.head eq outerContext)
      assert(enclosing.tail.head.macroApplication eq outerContext.macroApplication)
    } else {
      assert(contexts.size == 2)
      assert(enclosing.size == 1)
      assert(enclosing.head eq c)
    }
    assert(marker(c).isEmpty)
    c.internal.updateAttachment(c.macroApplication, Marker("inner"))
    assert(marker(c).contains(Marker("inner")))
    c.internal.removeAttachment[Marker](c.macroApplication)
    assert(marker(c).isEmpty)
    val symbol = c.macroApplication.symbol
    c.internal.updateAttachment(symbol, Marker("symbol"))
    assert(c.internal.attachments(symbol).get[Marker].contains(Marker("symbol")))
    c.internal.removeAttachment[Marker](symbol)
    assert(c.internal.attachments(symbol).get[Marker].isEmpty)
    val names = contexts.map(_.macroApplication.symbol.name.toString).mkString(",")
    val enclosingNames = enclosing.map(_.macroApplication.symbol.name.toString).mkString(",")
    c.Expr[String](Literal(Constant(names + "|" + enclosingNames)))
  }
}
