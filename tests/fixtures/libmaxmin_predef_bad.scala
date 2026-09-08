// The other half of `libmaxmin_predef.scala`: re-importing a source
// `scala.Predef` must not turn `Int` into a type that answers to anything.
//
// A widened import is only worth having if it still refuses what the program
// does not declare. `RichIntProbe` has `bigger` and `smaller`; `hugest` is
// declared nowhere, and the conversion cannot produce it. Real scalac 2.13.16
// rejects this file with `value hugest is not a member of Int`.
//
// `absent` is the second case: a name no conversion in scope reaches at all,
// so the view search has nothing even to try.
package scala {

  final class RichIntProbe(val self: Int) {
    def bigger(that: Int): Int = if (self > that) self else that
  }

  private[scala] abstract class LowPriorityProbe {
    implicit def probeWrapper(x: Int): RichIntProbe = new RichIntProbe(x)
  }

  object Predef extends LowPriorityProbe {
    type String = java.lang.String
  }
}

object Main {
  def main(args: Array[String]): Unit = {
    // `bigger` exists on RichIntProbe; `hugest` does not.
    val a: Int = 3 hugest 7
    // Nothing in scope converts an Int to anything with `absent`.
    val b: Int = 4.absent
    ()
  }
}
