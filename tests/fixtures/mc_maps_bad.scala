// `m(k) = v` is `m.update(k, v)`: the key and the value have to fit the
// declared parameter types, and a receiver without an `update` member is not
// assignable at all.
import scala.collection.mutable

class NoUpdate {
  def apply(i: Int): Int = i
}

object Main {
  def main(args: Array[String]): Unit = {
    val m = mutable.Map[String, Int]()
    m("a") = "wrong type"
    m(1) = 2

    val n = new NoUpdate
    n(0) = 7

    val q = mutable.Queue[Int]()
    q.enqueue("not an Int")
  }
}
