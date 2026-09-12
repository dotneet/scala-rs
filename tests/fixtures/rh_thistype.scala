// A call whose declared JVM result is weaker than the expression's own type.
// `buf += a` on a `ListBuffer[A]` has type `ListBuffer[A]` (`addOne` returns
// `this.type`) but resolves to `(Object)Lscala/collection/mutable/Growable;`, so
// nsc casts the result (`javap`: `invokevirtual $plus$eq; checkcast ListBuffer`).
// Without that cast, `if (p(a)) buf += a else buf` left a `Growable` at a join
// whose stack map says `ListBuffer` -- cats' `Foldable.filter_` / `toList` /
// `dropWhile_`, reached by the first use of `cats.implicits`.
//
// `s(a)` on a `Set[A]` is the other half of the same question from the other
// side: `SetOps.apply(A): Boolean` returns a *primitive*, and resolving that
// `apply` to `Function1`'s (whose result is a type parameter) inserted a
// `BoxesRunTime.unboxToBoolean` on an `int` -- "Bad type on operand stack: Type
// integer is not assignable to 'java/lang/Object'". cats'
// `TraverseFilter.ordDistinct` calls a `TreeSet` exactly this way.
import scala.collection.mutable
import scala.collection.immutable.TreeSet

object Main {
  def filter_[A](xs: List[A])(p: A => Boolean): List[A] =
    xs.foldLeft(mutable.ListBuffer.empty[A]) { (buf, a) =>
      if (p(a)) buf += a else buf
    }.toList

  def toList_[A](xs: List[A]): List[A] =
    xs.foldLeft(mutable.ListBuffer.empty[A]) { (buf, a) => buf += a }.toList

  def dropWhile_[A](xs: List[A])(p: A => Boolean): List[A] =
    xs.foldLeft(mutable.ListBuffer.empty[A]) { (buf, a) =>
      if (buf.nonEmpty || !p(a)) buf += a else buf
    }.toList

  def seen[A](alreadyIn: TreeSet[A], a: A): (TreeSet[A], Option[A]) =
    if (alreadyIn(a)) (alreadyIn, None) else (alreadyIn + a, Some(a))

  def member[A](s: Set[A], a: A): Boolean = if (s(a)) true else false

  def main(args: Array[String]): Unit = {
    println(filter_(List(1, 2, 3, 4))(_ % 2 == 0))
    println(toList_(List("a", "b")))
    println(dropWhile_(List(1, 2, 3, 1))(_ < 3))
    println(seen(TreeSet(1, 2), 2))
    println(seen(TreeSet(1, 2), 3))
    println(member(Set("a"), "a"))
    println(member(Set("a"), "b"))
  }
}
