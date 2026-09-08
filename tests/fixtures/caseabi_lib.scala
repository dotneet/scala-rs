// The scala-rs half of the case-class hashing ABI interop check. Real scalac
// 2.13.16 compiles `caseabi_app.scala` against the class files produced from
// this file, and the two halves then share one `HashMap`. See
// `crates/cli/tests/caseabi.rs`.
//
// The three case classes cover both of nsc's synthesized `hashCode` shapes:
// `Key` and `Tagged` have primitive accessors and get the inline MurmurHash3
// mix chain; `Refs` has none and forwards to `ScalaRunTime$._hashCode`.
//
// Every method here takes at least one argument on purpose. A zero-arg
// `def empty(): Empty` (and the `apply()` of a `case class Empty()`) is
// pickled without its empty parameter list, so real scalac reading our class
// files rejects the call site with "Empty does not take parameters" -- a
// separate, pre-existing pickling gap that has nothing to do with hashing and
// would otherwise stop this file from compiling at all.

case class Key(id: Int, name: String)
case class Refs(a: String, b: String)
case class Tagged(t: Long, f: Double, b: Boolean)

object Keys {
  def make(i: Int, n: String): Key = Key(i, n)
  def refs(a: String, b: String): Refs = Refs(a, b)
  def tagged(t: Long, f: Double, b: Boolean): Tagged = Tagged(t, f, b)
}
