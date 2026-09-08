// Admitting Java's default-access members from a class file must not make
// them accessible from another package. Real scalac 2.13.16 reports exactly
// these two, at these two lines.
package tmjava_other

import tmjava.JBase

object Bad {
  // Public: reachable from anywhere, and left alone.
  def ok: String = JBase.PUBLIC_TAG

  def bad1: String = JBase.SENTINEL

  def bad2: String = JBase.RESTART

  // `PROTOTYPE` is a `JBase[String, Integer]`, and only the field's
  // `Signature` attribute says so -- the descriptor is the raw `LJBase;`. A
  // compiler that reads only the descriptor accepts this, which is a
  // soundness hole rather than a missing type. It is public, so access has
  // nothing to do with it.
  def bad3: JBase[Integer, String] = JBase.PROTOTYPE
}
