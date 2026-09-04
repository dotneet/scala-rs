// scala/scala's `neg/t2918`, in one object: a higher-kinded type parameter
// bounded by an application of itself. scalac says
// `cyclic aliasing or subtyping involving type A`; scala-rs used to overflow
// its stack in `erasure::erase_ty`, which reported nothing at all.
object Main {
  def g[X, A[X] <: A[X]](x: A[X]): A[X] = x
  def main(args: Array[String]): Unit = ()
}
