package macrofixture

import scala.language.dynamics
import scala.language.experimental.macros
import scala.reflect.macros.whitebox

object MacroTypes extends Dynamic {
  def selectDynamic(name: String): Any = macro ParseBundle.carrier
  def expression(text: String): Any = macro ParseBundle.expression
}
class ParseBundle(val c: whitebox.Context) {
  import c.universe._
  def expression(text: Tree): Tree = {
    val Literal(Constant(source: String)) = text: @unchecked
    c.parse(source)
  }
  def carrier(name: Tree): Tree = {
    val Literal(Constant(source: String)) = name: @unchecked
    val constant = c.typecheck(c.parse(source)).tpe
    val carrier = c.typecheck(tq"{ type T = $constant; type F[A] = Later[A]; type Later[A] = List[A] }", mode = c.TYPEmode).tpe
    c.internal.setType(Literal(Constant(())), carrier)
  }
}
