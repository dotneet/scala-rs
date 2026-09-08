// The library half of `crates/cli/tests/implclassbin.rs`.
//
// Compiled by *real scalac*, so the pickle the app half reads is nsc's own.
// nsc expands each `implicit class C(x: T)` into a plain `class C` plus a
// `SYNTHETIC implicit def C(x: T): C`, and `SYNTHETIC` is what used to hide
// the conversion from the pickle reader -- leaving a method that is in scope
// under its own name but can never be selected as a view.
package iclib

class Target(val n: Int)

class Wide(t: Target) {
  def wide: String = "wide" + t.n
}

trait Prof {
  // A nested trait has no `ScalaSignature` of its own: its pickle lives on
  // `Prof.class`. Nothing adopts a class the program never writes by name,
  // and `import <a val>._` never writes this one.
  trait Api {
    implicit class RichTarget(t: Target) {
      def bump: Int = t.n + 1
    }
    // An ordinary `implicit def` next to it, which is not SYNTHETIC and so
    // fails for the other reason (the nested class is never adopted) rather
    // than for this one.
    implicit def widen(t: Target): Wide = new Wide(t)
  }
  val api: Api = new Api {}
}

object Holder extends Prof

// The same `implicit class` at the top level of an object, where the pickle
// *is* on the object's own class file: SYNTHETIC alone is enough to lose it.
object Flat {
  implicit class RichString(s: String) {
    def shout: String = s + "!"
  }
}
