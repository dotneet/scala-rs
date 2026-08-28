// Aligning the parent's type parameters with the child's must not weaken the
// bound check: `type C[T] = Int` still violates `type C[T] <: Bound[T]`.
trait Bound[T]
trait A { type C[T] <: Bound[T] }
trait B extends A { type C[T] = Int }
object Main { def main(args: Array[String]): Unit = println(0) }
