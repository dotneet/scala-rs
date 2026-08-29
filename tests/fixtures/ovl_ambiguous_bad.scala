// Neither alternative is more specific than the other, so the call is
// ambiguous. scalac: `ambiguous reference to overloaded definition`.
class Lit(val tpe: String)
object Lit {
  def apply(a: Int, b: Any): Lit = new Lit("a")
  def apply(a: Any, b: Int): Lit = new Lit("b")
}

object Main {
  def main(args: Array[String]): Unit = println(Lit(1, 2).tpe)
}
