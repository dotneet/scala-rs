// `Ordering[T]#compare` (and `lt`/`gt`/`lteq`/`gteq`/`equiv`/`max`/`min`) are
// `(T, T)`, not `(Any, Any)`.
object Main {
  def cmp[T](ord: Ordering[T], x: T, y: T): Int = ord.compare(x, y)
  def main(args: Array[String]): Unit = {
    println(Ordering[String].compare("a", "b"))
    println(Ordering[Int].compare(2, 1))
    println(cmp(Ordering[String], "b", "a"))
    println(cmp(Ordering[Int], 1, 1))
    println(Ordering[String].lt("a", "b"))
    println(Ordering[String].gt("a", "b"))
    println(Ordering[String].lteq("a", "a"))
    println(Ordering[String].gteq("a", "b"))
    println(Ordering[String].equiv("a", "a"))
    println(Ordering[String].max("a", "b"))
    println(Ordering[String].min("a", "b"))
  }
}
