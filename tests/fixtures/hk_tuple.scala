// Auto-tupling (SLS 6.6): an argument list that fits no alternative is
// retried packed into a single tuple. It is the last resort -- an alternative
// that takes the written number of arguments wins first (`h`), and once the
// list is tupled the usual most-specific rule picks among the alternatives
// (`b`).
//
// Nothing here prints a bare tuple: the private runtime's `Tuple2` has no
// `toString` of its own, so `println((1, "a"))` differs between the two
// modes for reasons that have nothing to do with tupling. `hk_tuple_lib`
// covers that shape against the real jar.
object Main {
  def f(t: (Int, String)): Int = t._1
  def s(t: (Int, String)): String = t._1.toString + t._2

  def h(a: Int, b: Int): String = "two-args"
  def h(t: (Int, Int)): String = "tupled"

  def a(): String = "a0"
  def a(x: Any): String = "aAny"

  def b(x: Any): String = "bAny"
  def b(t: (Int, String)): String = "bTup"

  def main(args: Array[String]): Unit = {
    println(f(1, "a"))
    println(s(3, "z"))
    println(h(1, 2))
    println(a(1, "x"))
    println(b(1, "x"))
  }
}
