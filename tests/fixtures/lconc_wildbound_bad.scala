// Lining the two wildcards' bounds up must not make any pair of bounded
// existentials conform: a different class in the bound, and a bound that pins
// the type argument, are both still rejected.
import java.util.Collection
import java.util.concurrent.Callable

object Main {
  class Other[X]
  def want[T](cs: Collection[_ <: Callable[T]]): Int = 0
  def wantInt(cs: Collection[_ <: Callable[Int]]): Int = 0

  def main(args: Array[String]): Unit = {
    val other: Collection[_ <: Other[String]] = new java.util.ArrayList[Other[String]]()
    println(want(other))
    val strs: Collection[_ <: Callable[String]] = new java.util.ArrayList[Callable[String]]()
    println(wantInt(strs))
  }
}
