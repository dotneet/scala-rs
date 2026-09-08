// nsc's `isOpAssignmentName` decides whether `x op= y` may be rewritten into
// `x = x op y`. It requires the operator's *first* character to be an operator
// character and **not** `=`, which is what keeps `===` (and `==>=`, `=:=`)
// out of the rewrite entirely. Everything below still has to work, and does:
// the rewrite fires for a real op-assignment on a `var`, on a field, on an
// array element and on a `Map` entry, and never for a name whose own member
// exists.
object Main {
  class Cell(var v: Int)

  // A user-defined `===` is an ordinary member and is called, not rewritten.
  implicit class Eq3(val a: Int) extends AnyVal {
    def ===(b: Int): String = if (a == b) "same" else "diff"
  }

  // `!==` *is* an op-assignment under nsc's rule (it starts with `!`), so
  // `x !== y` means `x = x != y` when no `!==` member exists.
  def main(args: Array[String]): Unit = {
    var n = 10
    n += 5
    n -= 2
    n *= 3
    println(n)

    var flags = 0
    flags |= 4
    flags &= 6
    flags ^= 1
    println(flags)

    val c = new Cell(7)
    c.v += 1
    println(c.v)

    val arr = Array(1, 2, 3)
    arr(1) += 10
    println(arr.mkString(","))

    val m = scala.collection.mutable.Map("k" -> 1)
    m("k") += 41
    println(m("k"))

    val sb = new StringBuilder("a")
    sb ++= "bc"
    println(sb.toString)

    var s = "x"
    s += "y"
    println(s)

    // Not a rewrite: `===` is a member here.
    println(3 === 3)
    println(3 === 4)

    var b = true
    b !== true // b = (b != true) -- an op-assignment, and `Boolean` has no `!==`
    println(b)
  }
}
