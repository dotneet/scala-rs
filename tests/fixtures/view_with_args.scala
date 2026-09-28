// A selection whose views cannot be told apart by the member's name is
// searched again with the argument types, as nsc's `adaptToMemberWithArgs`
// does: `a + b` on `T: Numeric` under `Numeric.Implicits._` sees both
// `infixNumericOps` and `Predef.any2stringadd`, and only the arguments say
// which `+` is meant.
package viewargs

import scala.math.Numeric.Implicits._
import scala.math.Ordering.Implicits._

object Ops {
  def add[T: Numeric](a: T, b: T): T = a + b
  def concat[T: Numeric](a: T): String = a + "!"
  def mixed[T: Numeric: Ordering](a: T, b: T): Boolean = (a + b) > (a * b) || a.abs < b
  def sum[T: Numeric](xs: List[T], z: T): T = xs.foldLeft(z)(_ + _)
}

object Main {
  def main(args: Array[String]): Unit = {
    println(Ops.add(1, 2))
    println(Ops.add(1.5, 2.25))
    println(Ops.concat(7))
    println(Ops.mixed(2, 3))
    println(Ops.sum(List(BigInt(3), BigInt(4)), BigInt(1)))
  }
}
