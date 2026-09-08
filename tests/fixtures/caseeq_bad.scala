// `canEqual` is synthesized for a `case` class only. A plain class that
// inherits the declaration is still abstract -- this is the very diagnostic
// that `src/library`'s `TupleN` used to get wrongly, and it has to keep
// firing where it is right.
//
// `MyEquals` stands in for `scala.Equals`: a *source* declaration, the only
// kind whose modifiers `override_check::modifiers_are_known` trusts.
//
// scalac 2.13.16 prints exactly what we print:
//   class Plain needs to be abstract.
//   Missing implementation for member of trait MyEquals:
//     def canEqual(that: Any): Boolean = ???
trait MyEquals {
  def canEqual(that: Any): Boolean
}

class Plain(val z: Int) extends MyEquals

object Main {
  def main(args: Array[String]): Unit = println(new Plain(1).z)
}
