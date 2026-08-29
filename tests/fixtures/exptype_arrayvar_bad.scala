// `Array` is invariant. scalac:
//   type mismatch; found: Array[Int]  required: Array[Any]
//   Note: Int <: Any, but class Array is invariant in type T.
object Main {
  def take(a: Array[Any]): Int = a.length

  def main(args: Array[String]): Unit = {
    val xs: Array[Int] = Array(1, 2)
    val ys: Array[Any] = xs
    println(take(xs))
    println(ys.length)
  }
}
