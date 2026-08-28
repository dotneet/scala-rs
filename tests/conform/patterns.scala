sealed trait Expr
case class Num(n: Int) extends Expr
case class Add(l: Expr, r: Expr) extends Expr
case class Mul(l: Expr, r: Expr) extends Expr
object Main {
  def eval(e: Expr): Int = e match {
    case Num(n) => n
    case Add(l, r) => eval(l) + eval(r)
    case Mul(l, r) => eval(l) * eval(r)
  }
  def show(x: Any): String = x match {
    case i: Int if i > 0 => "pos"
    case _: Int => "int"
    case s: String => s
    case (a, b) => a.toString + "-" + b.toString
    case h :: _ => "cons:" + h.toString
    case Nil => "nil"
    case _ => "other"
  }
  def main(args: Array[String]): Unit = {
    println(eval(Add(Num(1), Mul(Num(2), Num(3)))))
    println(show(1)); println(show(-1)); println(show("s"))
    println(show((1, 2))); println(show(List(9))); println(show(Nil))
    val (a, b) = (1, "x")
    println(a.toString + b)
    val p = Num(5)
    println(p.copy(n = 6))
    println(p == Num(5))
  }
}
