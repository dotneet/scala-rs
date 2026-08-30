// `Array.ofDim` is five alternatives that all take one type parameter, so
// `TypeApply` could not narrow the reference by them and the explicit
// `[Double]` reached nothing: the result stayed `Array[Array[T]]` and every
// use of an element reported `required: T`. Codegen then read the *inner*
// `Select`'s symbol, still the unresolved overload, and called the
// one-dimensional `ofDim(I, ClassTag)`; and its `[Ljava/lang/Object;` return
// needs the same narrowing `checkcast` scalac emits.
final class Cell(val v: Int) { override def toString: String = "Cell(" + v.toString + ")" }

object Main {
  def main(args: Array[String]): Unit = {
    val i1 = Array.ofDim[Int](2)
    i1(1) = 7
    println(i1(1)); println(i1.getClass.getName)
    val i2 = Array.ofDim[Int](2, 3)
    i2(1)(2) = 7
    println(i2(1)(2)); println(i2.getClass.getName)
    val i3 = Array.ofDim[Int](2, 3, 4)
    i3(1)(2)(3) = 7
    println(i3(1)(2)(3)); println(i3.getClass.getName)
    val i4 = Array.ofDim[Int](2, 2, 2, 2)
    i4(1)(1)(1)(1) = 7
    println(i4(1)(1)(1)(1)); println(i4.getClass.getName)
    val i5 = Array.ofDim[Int](2, 2, 2, 2, 2)
    i5(1)(1)(1)(1)(1) = 7
    println(i5(1)(1)(1)(1)(1)); println(i5.getClass.getName)

    val d1 = Array.ofDim[Double](2)
    d1(0) = 1.5
    d1(0) += 0.5
    println(d1(0)); println(d1.getClass.getName)
    val d2 = Array.ofDim[Double](2, 2)
    d2(0)(1) = 5.0
    d2(0)(1) += 1.0
    println(d2(0)(1)); println(d2.getClass.getName)
    val d3 = Array.ofDim[Double](2, 2, 2)
    d3(0)(1)(1) = 2.5
    println(d3(0)(1)(1)); println(d3.getClass.getName)

    val s1 = Array.ofDim[String](2)
    s1(0) = "a"
    s1(0) += "b"
    println(s1(0)); println(s1.getClass.getName)
    val s2 = Array.ofDim[String](2, 2)
    s2(1)(1) = "z"
    println(s2(1)(1)); println(s2.getClass.getName)

    val c1 = Array.ofDim[Cell](2)
    c1(0) = new Cell(3)
    println(c1(0)); println(c1.getClass.getName)
    val c2 = Array.ofDim[Cell](2, 2)
    c2(1)(0) = new Cell(4)
    println(c2(1)(0)); println(c2.getClass.getName)

    // An ascribed result and `Array.fill` already worked; they still do.
    val g: Array[Array[Int]] = Array.ofDim[Int](2, 3)
    g(1)(2) = 9
    println(g.map(_.mkString(",")).mkString(";"))
    val f = Array.fill(3)(0)
    f(1) += 2
    println(f(1)); println(f.getClass.getName)
    println(Array(1, 2, 3).mkString(","))
  }
}
