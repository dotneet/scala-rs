// The library half of the `bt_companion` pair: a class and its companion,
// compiled to *class files* and then reached from another compilation unit
// through `import btc._`.
//
// Two class files come out of each of these -- `btc/Holder.class` and
// `btc/Holder$.class` -- and only the first is installed when the package's
// member list is walked. That is what `bt_companion.scala` pins down.
package btc

class Holder[T](val v: T) {
  def get: T = v
  override def toString: String = "Holder(" + v + ")"
}

object Holder {
  def apply[T](v: T): Holder[T] = new Holder[T](v)
  val tag: String = "btc.Holder"
}

class Empty[T] {
  def name: String = "Empty"
}

/** The `TableQuery[E]` shape: a factory that takes no value parameters, so
  * `Empty[Int]` in term position is a complete expression. */
object Empty {
  def apply[T]: Empty[T] = new Empty[T]
}
