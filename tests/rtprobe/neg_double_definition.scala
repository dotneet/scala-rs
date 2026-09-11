// scalac: double definition: the two methods have the same type after
// erasure (List).
object Main {
  def f(l: List[Int]): Int = 0
  def f(l: List[String]): Int = 1
  def main(args: Array[String]): Unit = println(f(List(1)))
}
