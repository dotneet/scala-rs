// The rejecting side of `vf_alias`: a name that exists *only* in the type
// namespace is not a term, and a stable-id pattern that names it is an error
// rather than something to fall back on. Real scalac 2.13.16 reports
//
//   not found: value OnlyType
//   Identifiers that begin with uppercase are not pattern variables but match
//   the value in scope.
//
// The fix must not turn "no term of this name anywhere" into a silent
// `throw new RuntimeException("cannot load …")` again.
package vf

sealed abstract class HL2
class HC2(val h: Any) extends HL2
object HN2 extends HL2

object syn2 {
  type OnlyType = HN2.type
}

object Main {
  import syn2._

  def one(l: HL2): String = l match {
    case OnlyType => "nil"
    case _        => "other"
  }

  def main(args: Array[String]): Unit = println(one(HN2))
}
