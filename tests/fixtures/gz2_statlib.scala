// Compiled by **real scalac** into a directory, so the client below reads it
// the way it would read a jar: `Gz2Base.class` carries `public static
// java.lang.String mk(int)` and `public static java.lang.String tag()` --
// scalac's mirror of the companion object's members.
package gz2lib

class Gz2Base(val n: Int) {
  def own: String = "own" + n
}

object Gz2Base {
  def mk(n: Int): String = "mk" + n
  val tag: String = "T"
}
