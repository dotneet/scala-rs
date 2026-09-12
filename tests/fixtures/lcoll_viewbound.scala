// A member read through an implicit view that is a function *by inheritance*
// (`<:<` extends `From => To`), including one whose result applies a
// higher-kinded type parameter -- `scala/runtime/Tuple2Zipped.scala`'s
// `invert`, whose `x._1.iterator` goes through `w1: T1 <:< It1[El1]`.
object Main {
  class Ops[T1, T2](private val x: (T1, T2)) {
    // Exactly the library's shape. Typing the *body* is what this is for;
    // solving a higher-kinded `It1` at a call site is a separate gap, so the
    // runnable part below uses the first-order spelling.
    def sizes[El1, It1[a] <: Iterable[a], El2, It2[a] <: Iterable[a]](implicit
        w1: T1 <:< It1[El1],
        w2: T2 <:< It2[El2]
    ): (Int, Int) = {
      var n1 = 0
      val i1 = x._1.iterator
      while (i1.hasNext) { i1.next(); n1 += 1 }
      var n2 = 0
      val i2 = x._2.iterator
      while (i2.hasNext) { i2.next(); n2 += 1 }
      (n1, n2)
    }
    // The element types must come out as `El1` / `El2`, not as `Iterable`'s
    // own `A`: a second higher-kinded parameter spelling its argument `a` the
    // same way as the first used to take the first one's symbol, so the
    // declared `(El1, El2)` result did not accept the pair that was built.
    def firsts[El1, It1[a] <: Iterable[a], El2, It2[a] <: Iterable[a]](implicit
        w1: T1 <:< It1[El1],
        w2: T2 <:< It2[El2]
    ): (El1, El2) = (x._1.iterator.next(), x._2.iterator.next())
  }

  // `<:<` as a view, first order.
  def size[T, El](x: T)(implicit w: T <:< Iterable[El]): Int = {
    var n = 0
    val it = x.iterator
    while (it.hasNext) { it.next(); n += 1 }
    n
  }
  // A view written as a function type.
  def count[T](x: T)(implicit w: T => Iterable[Int]): Int = x.iterator.size
  // And `=:=`.
  def head[T](x: T)(implicit w: T =:= List[String]): String = x.head
  // A user class that inherits `Function1` is a view too.
  class Conv[-A, +B](f: A => B) extends (A => B) { def apply(a: A): B = f(a) }
  implicit val lift: Conv[Int, List[Int]] = new Conv(i => List(i, i + 1))
  def second[T](x: T)(implicit w: Conv[T, List[Int]]): Int = x.tail.head
  // Two higher-kinded parameters sharing the name of their own argument: the
  // members must be read at `It1`'s / `It2`'s own arguments.
  def both[El1, It1[a] <: Iterable[a], El2, It2[a] <: Iterable[a]](
      x: It1[El1],
      y: It2[El2]
  ): (El1, El2) = (x.iterator.next(), y.iterator.next())

  def main(args: Array[String]): Unit = {
    println(size(List(1, 2, 3)))
    println(count(List(7, 8, 9, 10)))
    println(head(List("x", "y")))
    println(second(41))
    println(both[Int, List, String, Vector](List(5, 6), Vector("p", "q")))
  }
}
