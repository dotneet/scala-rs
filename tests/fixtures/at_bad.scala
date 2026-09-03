// The rejections that come with `agent/asttype`'s relaxations.
//
//   * A *proper* type is still not a type constructor: `TC[Int]` where `TC`
//     takes `C[_]` is the kind error nsc reports
//     ("kinds of the type arguments (Int) do not conform ...").
//   * The wildcard only takes its parameter's kind inside a type *pattern*;
//     written as an ordinary type, `TC[_]` is an existential over a proper
//     type and nsc rejects it (`_$1 takes no type parameters, expected: 1`).
//   * Counting a bare `Select` as a recursive call does not make every
//     `@tailrec` legal: a parameterless recursive call in a non-tail position
//     is still "a recursive call not in tail position".
import scala.annotation.tailrec

abstract class TC[C[_]] {
  def sizeOf(c: C[Int]): Int
}

object Main {
  val wrong: TC[Int] = null
  def anyOf(t: TC[_]): Int = 0

  @tailrec
  def loop: Int = loop + 1

  def main(args: Array[String]): Unit = println(loop)
}
