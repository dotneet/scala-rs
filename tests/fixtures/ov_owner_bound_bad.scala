// The implementation narrows the bound after the owner is instantiated.
trait OwnerBound[T] {
  def apply[A <: T](a: A): A
}

object Main {
  val f: OwnerBound[AnyRef] = new OwnerBound[AnyRef] {
    def apply[A <: String](a: A): A = a
  }
}
