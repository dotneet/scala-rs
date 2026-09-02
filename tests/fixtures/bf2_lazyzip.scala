// `BuildFrom` matched at a higher kind: the only witness for a general
// collection is
//   buildFromIterableOps[CC[X] <: Iterable[X] with IterableOps[X, CC, _], A0, A]
//     : BuildFrom[CC[A0], A, CC[A]]
// and `LazyZip2.map[B, C](f)(implicit bf: BuildFrom[C1, B, C]): C` can only
// learn what `C` is from that witness.
import scala.collection.BuildFrom

object Main {
  // The user's own method with a `BuildFrom` clause: `C` is decided by the
  // search alone, exactly as in `LazyZip2.map`.
  def dup[C1, A, C](xs: C1, a: A)(implicit bf: BuildFrom[C1, A, C]): C = {
    val b = bf.newBuilder(xs)
    b += a
    b += a
    b.result()
  }

  def main(args: Array[String]): Unit = {
    // Fully applied: no unknown left for the search to solve.
    val ev = implicitly[BuildFrom[List[Int], String, List[String]]]
    println(ev.newBuilder(List(1)).result())

    val names = List("a", "b", "c")
    val sizes = List(1, 2, 3)

    // `C` is undetermined and only the witness pins it down.
    val joined = names.lazyZip(sizes).map((s, n) => s * n)
    println(joined)
    println(joined.mkString("|"))

    // The receiver's own collection comes back, not `Iterable`.
    val vs: Vector[String] = Vector("x", "y").lazyZip(Vector(2, 3)).map((s, n) => s * n)
    println(vs)

    val is: IndexedSeq[String] =
      IndexedSeq("p", "q").lazyZip(IndexedSeq(1, 2)).map((s, n) => s * n)
    println(is)

    // Three collections at once.
    val tri = List(1, 2).lazyZip(List(3, 4)).lazyZip(List(5, 6)).map((a, b, c) => a + b + c)
    println(tri)

    // `flatMap` takes the same evidence.
    val fm = List(1, 2).lazyZip(List(3, 4)).flatMap((a, b) => List(a, b))
    println(fm)

    // A `Set` receiver: `CC` is solved to `Set`, not to `Iterable`.
    val st: Set[Int] = Set(1, 2).lazyZip(Set(3, 4)).map((a, b) => a * b)
    println(st.toList.sorted)

    // The user's own clause, at two different collections.
    println(dup(List(1, 2), 7))
    println(dup(Vector("a"), "z"))
  }
}
