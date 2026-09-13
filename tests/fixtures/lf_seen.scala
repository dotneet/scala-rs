// As-seen-from must not substitute into the arguments it has just inserted.
// See `crates/cli/tests/lf.rs`.
//
// `Cell[T].v` is a `T`, and the receiver is a `Cell[Elem[A, C]]`, so `c.v` is an
// `Elem[A, C]`. The walk then reached `Elem` -- a base class of `Cell`, where
// `A` is `Elem[A, C]` itself and `C` is `Cell[Elem[A, C]]` -- and rewrote the
// `A` and `C` *inside the argument it had just put there*. The method is
// declared in `Elem`, so the free `A` and `C` are the very symbols that base
// class binds, which is what makes the capture possible at all.
trait Elem[A, C] {
  def readOne(c: Cell[Elem[A, C]]): Elem[A, C] = c.v
  def readTwo(cs: List[Cell[Elem[A, C]]]): Elem[A, C] = cs.head.v
  def readThree(m: Map[Int, Elem[A, C]]): Elem[A, C] = m.iterator.next()._2
}

class Cell[T](val v: T) extends Elem[T, Cell[T]] {
  override def toString: String = "Cell"
}

class UsesElem extends Elem[Int, String] {
  def one: String = readOne(new Cell[Elem[Int, String]](this)).toString
  def three: String = readThree(Map(1 -> (this: Elem[Int, String]))).toString
  override def toString: String = "UsesElem"
}

// The two library shapes the same root produced. Neither of them *reproduces*
// here -- in jar mode `Map`, `Builder` and `Seq` come from the prelude, whose
// `MapOps`/`IterableOps` hierarchy does not bind the enclosing trait's own
// symbols the way the library's sources do, and `tests/scalalib_measure.sh` is
// their real test. They are kept because they are what the fix was for and
// because they must keep compiling.
//
// `IterableOps.groupBy` writes `mutable.Map.empty[K, Builder[A, C]]` and then
// `m.iterator`, whose `MapOps[K, V, …]` declaration is `Iterator[(K, V)]`.
// `V := Builder[A, C]` is right; re-reading that `A` and `C` as `Map`'s own
// made `v.result()` a `Map` and `result.updated(k, v.result())` a
// `HashMap[K, AnyRef]` (`collection/Iterable.scala:570`).
import scala.collection.mutable
import scala.collection.immutable

trait GroupLike[A, C] {
  def newBuilder: mutable.Builder[A, C]
  def keysOf: List[(Int, A)]

  def group: immutable.Map[Int, C] = {
    val m = mutable.Map.empty[Int, mutable.Builder[A, C]]
    for ((k, a) <- keysOf) m.getOrElseUpdate(k, newBuilder) += a
    var result = immutable.HashMap.empty[Int, C]
    val mapIt = m.iterator
    while (mapIt.hasNext) {
      val (k, v) = mapIt.next()
      result = result.updated(k, v.result())
    }
    result
  }
}

class GroupInts extends GroupLike[Int, List[Int]] {
  def newBuilder: mutable.Builder[Int, List[Int]] = List.newBuilder[Int]
  def keysOf: List[(Int, Int)] = List((0, 2), (1, 3), (0, 4))
}

// The `unzip` shape: `SeqOps[A, CC, C]`'s own `A` read back through a receiver
// whose element type is `(A, Int)` came out `((A, Int), Int)`, one tuple too
// wide (`collection/Seq.scala:598,702`).
trait Perms[A] {
  def items: Seq[A]
  def split: (IndexedSeq[A], Array[Int]) = {
    val (es, is) = (items.map(e => (e, 1)).sortBy(_._2)).unzip
    val cs = new Array[Int](is.size)
    (es.toIndexedSeq, cs)
  }
}

class PermStrings extends Perms[String] {
  def items: Seq[String] = Seq("b", "a")
}

object Main {
  def main(args: Array[String]): Unit = {
    val u = new UsesElem
    println(u.one + " " + u.three)
    println(new GroupInts().group.toSeq.sortBy(_._1).mkString(";"))
    val (es, cs) = new PermStrings().split
    println(es.mkString(",") + " " + cs.length)
  }
}
