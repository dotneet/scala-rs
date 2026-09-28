import scala.language.experimental.macros
import scala.reflect.macros.blackbox

class ChainValue(val depth: Int) {
  def append(value: Int): ChainValue = macro ChainWork.append
  def duplicate: Int = macro ChainWork.duplicate
}

object ChainWork {
  def append(c: blackbox.Context)(value: c.Expr[Int]): c.Expr[ChainValue] = {
    import c.universe._
    val receiver = c.prefix.tree
    val statements = (0 until 256).map { index =>
      val name = TermName("item" + index)
      q"val $name = $index"
    }.toList
    c.Expr[ChainValue](q"{ ..$statements; new ChainValue($receiver.depth + ${value.tree}) }")
  }

  def duplicate(c: blackbox.Context): c.Expr[Int] = {
    import c.universe._
    val receiver = c.prefix.tree
    c.Expr[Int](q"$receiver.depth + $receiver.depth")
  }
}
