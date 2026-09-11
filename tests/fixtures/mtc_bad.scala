// Call sites for `mtc_bad_impl.scala`: every one of these is refused by
// scala-rs with a reason that names what the mirror could not answer.
// `docs/macros.md` §7.20.
//
// Real scalac 2.13.16 compiles and runs this file. The refusals are scala-rs's
// own limits, said out loud -- which is the point: an answer that was guessed
// at would compile here and be wrong, and nothing downstream would notice.
import scala.language.experimental.macros

object MtcBadUse {
  def noViews(): String = macro MtcBadImpl.noViewsImpl
  def noMacros(): String = macro MtcBadImpl.noMacrosImpl
  def withPt(): String = macro MtcBadImpl.withPtImpl
  def patternMode(): String = macro MtcBadImpl.patternModeImpl
  def caughtFailure(): String = macro MtcBadImpl.caughtFailureImpl
  def unbuildable(): String = macro MtcBadImpl.unbuildableImpl
}

object Main {
  def main(args: Array[String]): Unit = {
    println(MtcBadUse.noViews())
    println(MtcBadUse.noMacros())
    println(MtcBadUse.withPt())
    println(MtcBadUse.patternMode())
    println(MtcBadUse.caughtFailure())
    println(MtcBadUse.unbuildable())
  }
}
