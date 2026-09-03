// `agent/asttype`: the roots behind slick's `ast/Type.scala` and
// `compiler/RewriteJoins.scala` errors. Library-ABI only -- `@tailrec`,
// `Ordering`, `<:<` and `immutable.HashMap` all come from the real
// scala-library.
//
//  1. `Array` is a type *constructor* of kind `* -> *` even though source
//     `Array[T]` never becomes a class application, so
//     `TypedCollectionTypeConstructor[Array]` is a legal instantiation of
//     `class TC[C[_]]`. In a type *pattern*, `TC[_]` is legal too and takes
//     the kind its parameter asks for (`case o: …[?]` in slick).
//  2. `@tailrec` on a *parameterless* method: the recursive call
//     `n.sourceNominalType` is a bare `Select`, with no `Apply` round it.
//  3. `Ordering[Null]` exists only through
//     `Ordering.ordered[A](implicit asComparable: A => Comparable[A])`, whose
//     argument is `Predef.$conforms`; slick's `ScalaBaseType.nullType` needs
//     exactly that chain.
//  4. `immutable.HashMap`'s `filter`/`map`/`collect` are mixin forwarders in
//     the class file with no `Signature`; the typed declarations are the ones
//     its parents pickle. This is the `foundRefs.filter(_._2._2.isEmpty)`
//     shape from `RewriteJoins.hoistFilterFromBind`.
import scala.annotation.tailrec
import scala.collection.immutable

abstract class TC[C[_]] {
  def name: String
  def sizeOf(c: C[Int]): Int
  override def toString = name
  override def equals(o: Any) = o match {
    case t: TC[_] => t.name == name
    case _        => false
  }
  override def hashCode = name.hashCode
}

object TC {
  val forArray: TC[Array] = new TC[Array] {
    def name = "array"
    def sizeOf(c: Array[Int]) = c.length
  }
  val forList: TC[List] = new TC[List] {
    def name = "list"
    def sizeOf(c: List[Int]) = c.size
  }
}

final class Node(val next: Option[Node], val id: Int) {
  @tailrec
  def last: Node = next match {
    case Some(n) => n.last
    case None    => this
  }
  @tailrec
  final def depth(acc: Int): Int = next match {
    case Some(n) => n.depth(acc + 1)
    case None    => acc
  }
}

final class Sym(val n: Int) {
  override def toString = "s" + n
}

/// A `ConstArray`-shaped carrier: `toMap`'s key and value types are settled by
/// the `T <:< (R, U)` witness alone.
final class CA[T](val xs: List[T]) {
  def map[R](f: T => R): CA[R] = new CA(xs.map(f))
  def toMap[R, U](implicit ev: T <:< (R, U)): immutable.HashMap[R, U] = {
    val b = immutable.HashMap.newBuilder[R, U]
    xs.foreach(x => b += ev(x))
    b.result()
  }
}

object CA {
  def from[T](it: Iterable[T]): CA[T] = new CA(it.toList)
}

object Main {
  def main(args: Array[String]): Unit = {
    println(TC.forArray.sizeOf(Array(1, 2, 3)))
    println(TC.forList.sizeOf(List(1, 2)))
    println(TC.forArray.name + "/" + TC.forList.name)
    println(TC.forArray == TC.forArray)
    println(TC.forArray == TC.forList)

    val chain = new Node(Some(new Node(Some(new Node(None, 3)), 2)), 1)
    println(chain.last.id)
    println(chain.depth(0))

    val nullOrd: Ordering[Null] = implicitly[Ordering[Null]]
    println(nullOrd != null)
    val dateOrd = implicitly[Ordering[java.util.Date]]
    println(dateOrd.compare(new java.util.Date(0L), new java.util.Date(1L)))

    val sRefs: CA[(String, String)] = new CA(List(("p1", "b1"), ("p2", "b2")))
    val foundRefs =
      sRefs.map { case (p, onGen) => (p, (onGen, if (p == "p1") Some(new Sym(1)) else None)) }.toMap
    val newDefs = foundRefs.filter(_._2._2.isEmpty).map { case (p, (onGen, _)) => (p, (onGen, new Sym(9))) }
    val allRefs =
      foundRefs.collect { case (p, (_, Some(s))) => (p, s) } ++ newDefs.map { case (p, (_, s)) => (p, s) }
    val extra = CA.from(newDefs.map { case (_, (onGen, s)) => (s, onGen) })
    println(newDefs.size)
    println(allRefs.size)
    println(allRefs.get("p1").toString)
    println(extra.xs.size)
  }
}
