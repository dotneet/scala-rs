// The other direction for `lf_super.scala`: gathering one member set over the
// whole base type sequence must not start accepting more.
trait NoM { def other: Int = 1 }
trait AlsoNoM { def another: Int = 2 }

class NothingToCall extends NoM with AlsoNoM {
  // No parent has `m` at all.
  def a: Int = super.m
}

class NotAParent extends NoM {
  // `AlsoNoM` is not a parent of this class.
  def b: Int = super[AlsoNoM].another
}

// Two *genuine* overloads in unrelated mixins stay two: the override reduction
// must not collapse them just because both are concrete and the receiver's
// linearization orders their owners.
trait TakesString { def h(s: String): Int = 1 }
trait TakesInt { def h(n: Int): Int = 2 }
class Both extends TakesString with TakesInt {
  def c: Int = super.h(true)
}

// (`super` to a still-abstract member is nsc's RefChecks, which does not run
// once the typer has reported, so it cannot be asserted in this file.)
