// Override and inheritance rules scala/scala's own library relies on.
import java.{util => ju}

// A Java method's `Object` parameter matches a Scala `AnyRef` one (nsc
// `matchingParams`), both for `override` and for implementing an abstract
// Java method.
class JSet extends ju.AbstractSet[String] {
  override def remove(elem: AnyRef): Boolean = elem == "r"
  def iterator: ju.Iterator[String] = new ju.ArrayList[String]().iterator()
  def size = 0
}
class JDict extends ju.Dictionary[String, String] {
  def size = 0
  def isEmpty = true
  def keys: ju.Enumeration[String] = ju.Collections.emptyEnumeration()
  def elements: ju.Enumeration[String] = ju.Collections.emptyEnumeration()
  def get(k: AnyRef): String = "g"
  def put(k: String, v: String): String = v
  def remove(k: AnyRef): String = "r"
}

// A mixin trait whose superclass is a Java class the class's own Java
// superclass extends: `java.util.AbstractList` is an `AbstractCollection`.
trait IterWrap[A] extends ju.AbstractCollection[A] {
  val underlying: Iterable[A]
  def size = underlying.size
  override def iterator: ju.Iterator[A] = {
    val it = underlying.iterator
    new ju.Iterator[A] { def hasNext = it.hasNext; def next() = it.next() }
  }
}
class SeqWrap[A](val underlying: Seq[A]) extends ju.AbstractList[A] with IterWrap[A] {
  def get(i: Int) = underlying(i)
}

// `override` of a member only the trait's own self type has.
trait Sized { def size: Int; def isEmpty: Boolean = size == 0 }
trait Fixed { this: Sized =>
  override def size: Int = 1
  override def isEmpty: Boolean = false
}
class Both extends Sized with Fixed

// A member trait's self type, read at the class that mixes it in: `K` is
// `MapLike`'s in `GenKeys`, and `SortedMapLike`'s in `SortedKeys`.
trait KSet[A] { def has(a: A): Boolean }
trait KSortedSet[A] extends KSet[A]
trait MapLike[K] {
  def keyList: List[K]
  protected trait GenKeys { this: KSet[K] => def count: Int = keyList.size }
  protected class Keys extends KSet[K] with GenKeys { def has(k: K) = keyList.contains(k) }
}
trait SortedMapLike[K] extends MapLike[K] {
  protected class SortedKeys extends KSortedSet[K] with GenKeys { def has(k: K) = keyList.contains(k) }
  protected class MoreKeys extends Keys
  def sortedKeys: KSortedSet[K] = new SortedKeys
  def keyCount: Int = (new SortedKeys).count + (new MoreKeys).count
}
class IntMap(val keyList: List[Int]) extends SortedMapLike[Int]

// A self alias is not a member: `s.self` below is the private val, not
// `Fn`'s alias (scala/scala's `WrappedString` against `Function1`).
trait Fn[-T, +R] { self =>
  def apply(t: T): R
  def andThen2[A](g: R => A): T => A = (t: T) => g(self(t))
}
final class Wrapped(private val self: String) extends Fn[Int, Char] {
  def apply(i: Int): Char = self.charAt(i)
  def same(that: Any): Boolean = that match {
    case s: Wrapped => self == s.self
    case _          => false
  }
}

object Main {
  def main(args: Array[String]): Unit = {
    println((new JSet).remove("r"))
    println((new JDict).get("x") + (new JDict).remove("y"))
    val w = new SeqWrap(Seq(1, 2, 3))
    println((w.size, w.get(1), w.iterator.next()))
    println(((new Both).size, (new Both).isEmpty))
    val m = new IntMap(List(1, 2))
    println((m.sortedKeys.has(2), m.keyCount))
    println((new Wrapped("ab").same(new Wrapped("ab")), new Wrapped("ab").andThen2(_.toInt)(1)))
  }
}
