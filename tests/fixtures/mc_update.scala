// SLS 6.15: `f(args) = v` is `f.update(args, v)` for *any* receiver with an
// `update` member -- an array, a user class, a curried result, a field. Runs
// on the private runtime too: nothing here is library-backed.
class Box {
  var v: Int = 0
  def update(i: Int, x: Int): Unit = v = x + i
}

class Grid {
  var last: String = ""
  def update(i: Int, j: Int, x: String): Unit = last = s"$i:$j:$x"
}

class Holder {
  val b = new Box
}

class Poly[T] {
  var v: String = ""
  def update(i: Int, x: T): Unit = v = s"$i=$x"
}

// `update` may return something other than Unit; the value is discarded.
class Counting {
  var n = 0
  def update(i: Int, x: Int): Int = { n = n + i + x; n }
}

object Main {
  def boxes(h: Holder): Box = h.b

  def main(args: Array[String]): Unit = {
    val a = new Array[Int](3)
    a(0) = 7
    a(2) = a(0) + 1
    println(a(0) + "," + a(1) + "," + a(2))

    val b = new Box
    b(10) = 5
    println(b.v)

    val g = new Grid
    g(1, 2) = "hi"
    println(g.last)

    // The receiver is a selection, not a local.
    val h = new Holder
    h.b(1) = 41
    println(h.b.v)

    val p = new Poly[String]
    p(3) = "x"
    println(p.v)

    val c = new Counting
    c(1) = 2
    c(3) = 4
    println(c.n)

    // Nested: the receiver of the outer `update` is itself an `apply`.
    val outer = new Holder
    boxes(outer)(2) = 3
    println(outer.b.v)
  }
}
