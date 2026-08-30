// The fix in `agent/setapply` is shape-exact: a pickle-derived member is only
// declined when its erased parameters exactly match an existing
// hand-written prelude member already on the same class. Two genuinely
// different `apply` overloads that a call cannot choose between must still
// be `ambiguous overload`, exactly as before.
trait Ax
trait Bx
class Cx extends Ax with Bx

object Pick {
  def apply(x: Ax): String = "a"
  def apply(x: Bx): String = "b"
}

object Main {
  def main(args: Array[String]): Unit = {
    println(Pick(new Cx))
  }
}
