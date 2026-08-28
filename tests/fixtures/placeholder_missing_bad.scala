// nsc: `missing parameter type for expanded function` — nothing determines the
// placeholder's type here. The expected type is unknown and `_` is the receiver
// of the call, not one of its arguments, so the callee's signature cannot fill
// it in either (unlike `two(_, 3)`, which is legal).
object Main {
  val f = _ + 1
  def main(args: Array[String]): Unit = println(f(1))
}
