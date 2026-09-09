// Implicit-view priority by *where the conversion is defined*.
//
// SLS 6.26.3 breaks a tie between two applicable implicit conversions partly
// by their owners: one defined in a class that the other's owner inherits
// from is the weaker of the pair. The standard library is built on this --
// `object Predef extends LowPriorityImplicits`, with `augmentString` on
// `Predef` and `wrapString` on the base class -- which is why `"abc".slice`
// is `StringOps#slice` and not `WrappedString#slice`.
//
// Before `agent/strarrayops` this compiler scored such a pair equal and gave
// up, reporting `value sel is not a member of String`.
import scala.language.implicitConversions

class HighOps(val s: String) {
  def sel: String = "high"
}
class LowOps(val s: String) {
  def sel: String = "low"
  def onlyLow: String = "onlyLow"
}

abstract class LowPrio {
  implicit def toLow(s: String): LowOps = new LowOps(s)
}
object Conv extends LowPrio {
  implicit def toHigh(s: String): HighOps = new HighOps(s)
}

// Three owners deep, so the rule is transitive and not one step.
class TopOps(val s: String) { def three: String = "top" }
class MidOps(val s: String) { def three: String = "mid" }
class BotOps(val s: String) { def three: String = "bot" }

abstract class L3 {
  implicit def c3(s: String): BotOps = new BotOps(s)
}
abstract class L2 extends L3 {
  implicit def c2(s: String): MidOps = new MidOps(s)
}
object Conv3 extends L2 {
  implicit def c1(s: String): TopOps = new TopOps(s)
}

// A trait, mixed in rather than extended, and the conversion on the *object*
// still wins.
class MixOps(val s: String) { def mix: String = "mixObject" }
class TraitOps(val s: String) { def mix: String = "mixTrait" }
trait MixPrio {
  implicit def toTrait(s: String): TraitOps = new TraitOps(s)
}
object ConvMix extends MixPrio {
  implicit def toMixObject(s: String): MixOps = new MixOps(s)
}

// The conversion the *base* declares is still used when it is the only one
// that offers the member. The rule demotes; it must not remove.
object Main {
  def declaredBeatsInherited: String = {
    import Conv._
    "x".sel
  }
  def inheritedIsStillUsed: String = {
    import Conv._
    "x".onlyLow
  }
  def nearestOfThree: String = {
    import Conv3._
    "x".three
  }
  def objectBeatsMixedInTrait: String = {
    import ConvMix._
    "x".mix
  }
  def main(args: Array[String]): Unit = {
    println(declaredBeatsInherited)
    println(inheritedIsStillUsed)
    println(nearestOfThree)
    println(objectBeatsMixedInTrait)
  }
}
