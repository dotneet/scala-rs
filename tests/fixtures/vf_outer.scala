// A super-constructor argument that reads the *enclosing* instance.
//
// `new PR(rs) { … }` written inside `PR` builds an anonymous subclass whose
// constructor takes the enclosing `PR` as its first parameter. The argument
// list belongs to the enclosing class, so `rs` means `outer.rs` -- and in the
// pre-super region slot 0 is `uninitializedThis`, which JVMS §4.10.1.9 lets
// `putfield` take and nothing else. Reading it off `this` was both the wrong
// object and a `VerifyError: Bad type on operand stack … Type
// uninitializedThis … is not assignable to 'PR'`
// (slick's `PositionedResult.view`, class `PositionedResult$$anon$507`).
package vf

abstract class PR(val rs: String) { outer =>
  protected[this] var pos = 0
  def label: String
  def tag: String = "PR(" + rs + ")"

  // Reads a `val` of the enclosing instance.
  def view(d: Int): PR = new PR(rs) {
    pos = d
    def label = "inner:" + outer.label + ":" + pos
  }

  // Reads a *method* of the enclosing instance, which is the same rule for
  // `invokevirtual` as for `getfield`.
  def view2(d: Int): PR = new PR(tag) {
    pos = d
    def label = "inner2:" + rs + ":" + pos
  }

  // A named local class, not an anonymous one.
  def view3(d: Int): PR = {
    class Local extends PR(rs) {
      pos = d
      def label = "local:" + rs + ":" + pos
    }
    new Local
  }
}

class PRImpl(s: String) extends PR(s) {
  def label = "outer:" + rs
}

object Main {
  def main(args: Array[String]): Unit = {
    val p = new PRImpl("a")
    println(p.label)
    println(p.view(3).label)
    println(p.view(3).rs)
    println(p.view2(4).label)
    println(p.view3(5).label)
  }
}
