// A method bound may refer to the type argument of its owner.
trait BoundApply[T] {
  def apply[A <: T](a: A): A
}

object Main {
  val f: BoundApply[CharSequence] = new BoundApply[CharSequence] {
    def apply[A <: CharSequence](a: A): A = a
  }

  def main(args: Array[String]): Unit = println(f("owner-bound"))
}
