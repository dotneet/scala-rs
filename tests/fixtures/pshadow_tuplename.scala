// `TupleN` is a type name. A *term* of the same name in a nearer scope must
// not answer for it: `scala.math.Equiv` declares `implicit def Tuple2[T1, T2]`
// and then writes `x._1` on a `(T1, T2)` in the same object, which reported
// `value _1 is not a member of (T1, T2)` -- the lookup stopped at the method.
object Fake {
  def Tuple2(n: Int): String = "method Tuple2(" + n + ")"
  def Tuple3(n: Int): String = "method Tuple3(" + n + ")"

  def first[A, B](p: (A, B)): A = p._1
  def second[A, B](p: (A, B)): B = p._2
  def mid[A, B, C](t: (A, B, C)): B = t._2

  // The type name still means the class, even here.
  def pair[A, B](p: Tuple2[A, B]): A = p._1
}

object Main {
  def main(args: Array[String]): Unit = {
    println(Fake.Tuple2(1))
    println(Fake.Tuple3(2))
    println(Fake.first(("a", 3)))
    println(Fake.second(("a", 3)))
    println(Fake.mid((1, "b", 2.0)))
    println(Fake.pair(("k", 9)))
  }
}
