import scala.language.experimental.macros
import scala.reflect.macros.blackbox

final class Chain {
  def append[A]: Chain = macro ChainImpl.append[A]
}

object ChainImpl {
  def append[A: c.WeakTypeTag](c: blackbox.Context): c.Expr[Chain] = {
    import c.universe._
    val statement = q"val ${TermName(c.freshName("entry"))}: Int = 1"
    val tree = c.prefix.tree match {
      case Block(stats, result) => Block(stats :+ statement, result)
      case other => Block(List(statement), other)
    }
    c.Expr[Chain](tree)
  }
}
