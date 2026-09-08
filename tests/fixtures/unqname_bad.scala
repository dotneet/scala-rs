// The two rejections `agent/unqualname`'s roots have to keep making.
//
// Supplying `AnyRef`'s monitor methods must not make *any* argument list
// acceptable, and shadowing a builtin type name has to actually take effect:
// if `Int` here still meant `scala.Int`, `x + 1` would compile.
//
// Real scalac 2.13.16 rejects both lines too (`found String("soon") required
// Long`, and `found Int(1) required String` -- `+` on a non-numeric operand
// is `String`'s).

class Bad {
  def a(): Unit = synchronized { wait("soon") }
}

object Shadow {
  trait Int { def zero(i: scala.Int): Boolean = i == 0 }
  def arith(x: Int): scala.Int = x + 1
}
