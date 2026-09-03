// The other side of "a rigid type parameter argument is only what its bounds
// say": scoring one applicable to every parameter is what made
// `String.valueOf(r)` ambiguous, and it also accepted this, which real
// scalac 2.13.16 rejects with
// `type mismatch; found: T  required: List[Int]`.
object Main {
  def takesList(l: List[Int]): Int = l.size
  def bad[T](x: T): Int = takesList(x)
  def main(args: Array[String]): Unit = println(bad(1))
}
