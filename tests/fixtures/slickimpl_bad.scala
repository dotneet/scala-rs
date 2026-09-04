// The flip side of the two rules `slickimpl.scala` relies on.
//
// A deferred `val` is implemented by a matching definition anywhere in the
// linearization -- but a declaration that is *itself* an `override` takes an
// implementation away, and only a base above it can put one back. And a
// definition that does not match implements nothing, self type or not.

class B10 { def f: Int = 1 }
abstract class M10 extends B10 { override def f: Int }
class C10 extends M10

trait A10 { val n: Int }
trait P10 { self: A10 => lazy val m: Int = 3 }
object D10 extends P10 with A10
