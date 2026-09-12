// Programs scalac rejects that the repairs in `erascg` must keep rejecting.
import scala.collection.immutable.TreeMap

class NoOrd
class NA(a: Int, b: String) { def this(s: String) = this(1, s) }

object Main {
  def sum(xs: Int*): Int = xs.sum
  def f[T](xs: T*): T = xs.head
  def main(args: Array[String]): Unit = {
    val h = sum _
    println(h(1, 2))
    println(new NA)
    println(new TreeMap[NoOrd, String])
    val k: Seq[String] => String = f
  }
}
