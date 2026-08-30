// `anewarray`'s operand is an *internal name*, and for an array component that
// name is the descriptor itself (`[I`). Emitting `java/lang/Object` for every
// non-class element gave `new Array[Array[Int]](n)` the erasure
// `[Ljava/lang/Object;`, and the first `arr(i)(j)` failed verification with
//   java.lang.VerifyError: Bad type on operand stack in iaload
object Main {
  def sum(a: Array[Array[Int]]): Int = {
    var t = 0
    var i = 0
    while (i < a.length) {
      var j = 0
      while (j < a(i).length) { t += a(i)(j); j += 1 }
      i += 1
    }
    t
  }

  def main(args: Array[String]): Unit = {
    val n = 2

    val a = new Array[Array[Int]](n)
    a(0) = new Array[Int](2)
    a(0)(0) = 1
    a(0)(1) = 2
    a(1) = new Array[Int](2)
    a(1)(0) = 3
    a(1)(1) = 4
    println(a(0)(1))
    println(sum(a))
    println(a.getClass.getName)

    val s = new Array[Array[String]](2)
    s(0) = new Array[String](2)
    s(0)(0) = "x"
    s(0)(1) = "y"
    println(s(0)(1))
    println(s.getClass.getName)

    val t3 = new Array[Array[Array[Int]]](1)
    t3(0) = new Array[Array[Int]](1)
    t3(0)(0) = new Array[Int](1)
    t3(0)(0)(0) = 7
    println(t3(0)(0)(0))
    println(t3.getClass.getName)

    // One-dimensional arrays keep their `newarray` / `anewarray` forms.
    val p = new Array[Int](3)
    p(2) = 9
    println(p(2))
    println(p.getClass.getName)

    val st = new Array[String](1)
    st(0) = "s"
    println(st(0))
    println(st.getClass.getName)

    val o = new Array[Object](1)
    println(o.getClass.getName)

    // Tuples and functions erase to their own classes, not to `Object`.
    val tu = new Array[(Int, Int)](1)
    tu(0) = (1, 2)
    println(tu(0)._1.toString + "," + tu(0)._2.toString)
    println(tu.getClass.getName)

    val fn = new Array[Int => Int](1)
    fn(0) = (q: Int) => q + 1
    println(fn(0)(1))
    println(fn.getClass.getName)
  }
}
