// A member read through a *compound* upper bound is read at the merged
// (glb) instantiation of the base class, not at whichever parent is written
// first -- `trait LinearSeqOps[+A, +CC[X] <: LinearSeq[X], +C <: LinearSeq[A]
// with LinearSeqOps[A, CC, C]]`, where `these = these.tail` has to be a `C`.
object Main {
  trait LinSeq[+A] extends LinOps[A, LinSeq, LinSeq[A]]
  trait LinOps[+A, +CC[X] <: LinSeq[X], +C <: LinSeq[A] with LinOps[A, CC, C]] {
    def tail: C
    def isEmpty: Boolean
    def coll: C
    def count: Int = {
      var these = coll
      var n = 0
      while (!these.isEmpty) { n += 1; these = these.tail }
      n
    }
  }
  final class L[A](val xs: List[A]) extends LinSeq[A] with LinOps[A, LinSeq, L[A]] {
    def tail: L[A] = new L(xs.tail)
    def isEmpty: Boolean = xs.isEmpty
    def coll: L[A] = this
  }

  // `this.type` on a receiver that *applies* a higher-kinded parameter:
  // `scala/collection/mutable/SortedMap.scala`'s
  // `clone().asInstanceOf[CC[K, V1]].addOne((key, value))`.
  trait Buf[A] { def add(a: A): this.type = this }
  class Bag[A] extends Buf[A]
  def addTo[A, CC[x] <: Buf[x]](c: CC[A], x: A): CC[A] = c.add(x)

  def main(args: Array[String]): Unit = {
    println(new L(List(1, 2, 3, 4)).count)
    println(addTo[Int, Bag](new Bag[Int], 1).getClass.getSimpleName)
  }
}
