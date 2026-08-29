// A companion `apply` is still checked against its parameter types: the
// default on the third parameter does not make the first two optional, and it
// does not turn `Int` into `String`. scalac: `overloaded method value apply
// with alternatives … cannot be applied to (Int, Int, Int)`.
class Lit(val tpe: String, val value: Any, val volatileHint: Boolean)
trait Tagged[T] { def label: String }
object Tagged {
  implicit val intTagged: Tagged[Int] = new Tagged[Int] { def label = "int" }
}
object Lit {
  def apply(tpe: String, value: Any, volatileHint: Boolean = false): Lit =
    new Lit(tpe, value, volatileHint)
  def apply[T](value: T)(implicit t: Tagged[T]): Lit =
    new Lit(t.label, value, false)
}

object Main {
  def main(args: Array[String]): Unit = println(Lit(1, 2, 3).tpe)
}
