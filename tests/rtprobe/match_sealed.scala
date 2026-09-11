// Sealed hierarchies, case objects, nested case class patterns, stable
// identifier patterns (backquoted and uppercase), and constant patterns.
object Main {
  sealed trait Expr
  case class Num(v: Int) extends Expr
  case class Add(l: Expr, r: Expr) extends Expr
  case class Mul(l: Expr, r: Expr) extends Expr
  case class Neg(e: Expr) extends Expr
  case object Zero extends Expr

  def eval(e: Expr): Int = e match {
    case Num(v) => v
    case Add(l, r) => eval(l) + eval(r)
    case Mul(Num(0), _) | Mul(_, Num(0)) => 0
    case Mul(l, r) => eval(l) * eval(r)
    case Neg(Neg(x)) => eval(x)
    case Neg(x) => -eval(x)
    case Zero => 0
  }
  def simplify(e: Expr): Expr = e match {
    case Add(Num(0), x) => simplify(x)
    case Add(x, Num(0)) => simplify(x)
    case Mul(Num(1), x) => simplify(x)
    case Add(l, r) => Add(simplify(l), simplify(r))
    case other => other
  }

  final val Limit = 10
  val threshold = 5
  def stable(x: Int, expected: Int): String = x match {
    case `expected` => "as expected"
    case Limit => "at limit"
    case `threshold` => "at threshold"
    case _ => "other"
  }

  sealed abstract class Color(val rgb: Int)
  case object Red extends Color(0xff0000)
  case object Green extends Color(0x00ff00)
  case object Blue extends Color(0x0000ff)
  def name(c: Color): String = c match { case Red => "red"; case Green => "green"; case Blue => "blue" }

  def main(args: Array[String]): Unit = {
    val e = Add(Num(2), Mul(Num(3), Neg(Neg(Num(4)))))
    println(e); println(eval(e))
    println(eval(Mul(Num(0), Num(99))) + " " + eval(Neg(Num(3))) + " " + eval(Zero))
    println(simplify(Add(Num(0), Add(Mul(Num(1), Num(5)), Num(0)))))
    println(stable(3, 3) + " " + stable(10, 3) + " " + stable(5, 3) + " " + stable(7, 3))
    println(List(Red, Green, Blue).map(c => name(c) + "=" + Integer.toHexString(c.rgb)))
    println(Zero.toString + " " + Red.productPrefix + " " + (Red == Red) + " " + Red.hashCode.==(Red.hashCode))
    println((Zero: Any) match { case z: Expr => "expr " + z; case _ => "?" })
  }
}
