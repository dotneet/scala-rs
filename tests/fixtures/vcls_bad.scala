package hl

trait Univ extends Any {
  def describe: String
}

final class Meters(val n: Int) extends AnyVal with Univ {
  def describe = n + "m"
}

object Bad {
  // The underlying value is not visible through the universal trait.
  def viaTrait(u: Univ): Int = u.n

  def missing(m: Meters): Int = m.missingMember

  // `notStable` is a method, so it cannot start a singleton type.
  def notStable: Meters = new Meters(1)
  def bogus: notStable.type = notStable
}
