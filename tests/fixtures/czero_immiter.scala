// `scala.collection.immutable.Iterable` is a supertype of the immutable
// library. In 2.13 the three unsorted immutable traits name it as their first
// parent (`trait Seq[+A] extends Iterable[A] with collection.Seq[A]`, and the
// same for `Set` and `Map[K, V] extends Iterable[(K, V)]`), and `import
// scala.collection.immutable._` makes the bare name mean it -- which is how
// cats' `NonEmptySet.scala` writes
// `override def toIterable[A](fa: NonEmptySet[A]): Iterable[A] = fa.toSortedSet`.
// Only the `collection.*` half of each edge was wired in the prelude, so
// nothing in the immutable library conformed to it at all.
import scala.collection.immutable._

object Main {
  type II[A] = Iterable[A]

  def l[A](x: List[A]): II[A] = x
  def v[A](x: Vector[A]): II[A] = x
  def s[A](x: Set[A]): II[A] = x
  def ss[A](x: SortedSet[A]): II[A] = x
  def ts[A](x: TreeSet[A]): II[A] = x
  def m[A](x: Map[A, A]): II[(A, A)] = x
  def sm[A](x: SortedMap[A, A]): II[(A, A)] = x
  def tm[A](x: TreeMap[A, A]): II[(A, A)] = x
  def q[A](x: Queue[A]): II[A] = x
  def ll[A](x: LazyList[A]): II[A] = x
  def r(x: Range): II[Int] = x
  def bs(x: BitSet): II[Int] = x
  def isq[A](x: Seq[A]): II[A] = x
  def iis[A](x: IndexedSeq[A]): II[A] = x
  def hs[A](x: HashSet[A]): II[A] = x
  def hm[A](x: HashMap[A, A]): II[(A, A)] = x

  // The same conformance where the subtype arrives through a type parameter
  // rather than written out.
  def thru[C[x] <: Seq[x], A](x: C[A]): II[A] = x

  def main(args: Array[String]): Unit = {
    println(l(List(1, 2)).size)
    println(v(Vector(1, 2, 3)).size)
    println(s(Set(1)).size)
    println(ss(SortedSet(3, 1, 2)).toList)
    println(ts(TreeSet(5, 4)).toList)
    println(m(Map(1 -> 1)).size)
    println(sm(SortedMap(2 -> 2, 1 -> 1)).toList)
    println(tm(TreeMap(1 -> 9)).toList)
    println(q(Queue(7, 8)).toList)
    println(ll(LazyList(1, 2, 3)).size)
    println(r(1 to 4).toList)
    println(bs(BitSet(1, 3)).toList)
    println(isq(Seq(1, 2)).size)
    println(iis(IndexedSeq(1, 2, 3)).size)
    println(hs(HashSet(1, 2)).size)
    println(hm(HashMap(1 -> 1)).size)
    println(thru(List(1, 2, 3)).size)
  }
}
