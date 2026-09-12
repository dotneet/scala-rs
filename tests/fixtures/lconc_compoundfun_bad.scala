// Reaching a compound's `apply` through its parents must not make every
// compound callable, nor forget the arity of the `apply` it found.
object Main {
  trait Marker { def block(): Boolean }

  def bad1[T](t: T): T = {
    val b: Marker with (() => T) = new Marker with (() => T) {
      def block(): Boolean = true
      def apply(): T = t
    }
    b(1)
  }

  def bad2(): AnyRef = {
    val m: Marker with Cloneable = new Marker with Cloneable { def block() = true }
    m(1)
  }
}
