// Binding a value-class scrutinee straight through must not loosen what the
// pattern is allowed to say. Real scalac 2.13.16 rejects all of these too.
final case class Wrapped(u: Int) extends AnyVal

object Bad {
  // Wrong arity: `Wrapped` has one field.
  def two(w: Wrapped): Int = w match { case Wrapped(x, y) => x + y }

  // `unapply` takes a `Wrapped`, and an `Int` is not one even though the two
  // share an erasure.
  val direct: Option[Int] = Wrapped.unapply(3)
}
