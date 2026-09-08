// The synthesized `hashCode` steps aside for a hand-written one -- but only a
// well-typed hand-written one. `Object.hashCode` returns `Int`, so this
// override has to be rejected rather than silently replaced by the
// MurmurHash3 body the case class would otherwise have got.

case class Bad(x: Int) {
  override def hashCode(): String = "nope"
}

object Main {
  def main(args: Array[String]): Unit = println(Bad(1).hashCode)
}
