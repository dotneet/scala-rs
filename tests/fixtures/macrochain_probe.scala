import scala.language.experimental.macros
import scala.reflect.macros.blackbox

class ChainProbe(val value: Int) {
  def next[A]: ChainProbe = macro ChainProbeMacros.next[A]
}
object ChainProbeMacros {
  def next[A: c.WeakTypeTag](c: blackbox.Context): c.Expr[ChainProbe] = {
    import c.universe._
    println("chain-probe-expanded")
    c.Expr[ChainProbe](q"new ChainProbe(${c.prefix}.value + 1)")
  }
}
