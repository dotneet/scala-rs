// `private[p]` still stops at the package boundary, and a constructor
// parameter with no inherited namesake is still not a member.
package mism8bad {
  class Holder {
    private[mism8bad] val slot: Int = 1
  }
  class Local(y: Int) {
    def get: Int = y
  }
}

object Use {
  def a(h: mism8bad.Holder): Int = h.slot
  def b(l: mism8bad.Local): Int = l.y
}
