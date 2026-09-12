// Semigroup / Monoid, including the two instances whose shape is hardest to
// get right: Map (which needs the value Semigroup) and tuples (which need one
// per component).
import cats._
import cats.kernel.{Comparison, Order => KOrder}
import cats.syntax.all._

import scala.collection.immutable.SortedSet

object Main {
  def main(args: Array[String]): Unit = {
    println(Monoid[Int].empty)
    println(Monoid[Int].combine(2, 3))
    println(Monoid[String].combine("a", "b"))
    println(Monoid[String].empty == "")
    println(Semigroup[Int].combine(4, 5))
    println(Monoid[List[Int]].combine(List(1), List(2, 3)))
    println(Monoid[Option[Int]].combine(Some(1), Some(2)))
    println(Monoid[Option[Int]].combine(Some(1), None))
    println(Monoid[Option[String]].combine(Some("a"), Some("b")))

    // Map: combine merges and combines colliding values
    println(Monoid[Map[String, Int]].combine(Map("a" -> 1), Map("a" -> 2, "b" -> 3)))
    println(Monoid[Map[String, Int]].empty)
    println(Monoid[Map[String, List[Int]]].combine(Map("k" -> List(1)), Map("k" -> List(2))))
    println(Map("a" -> 1) |+| Map("a" -> 2, "c" -> 4))

    // tuples
    println(Monoid[(Int, String)].combine((1, "a"), (2, "b")))
    println(Monoid[(Int, String, List[Int])].combine((1, "a", List(1)), (2, "b", List(2))))
    println(Monoid[(Int, String)].empty)
    println(((1, "a")) |+| ((2, "b")))

    // combineAll / fold
    println(Monoid[Int].combineAll(List(1, 2, 3)))
    println(Monoid[String].combineAll(List("x", "y")))
    println(Monoid[Map[String, Int]].combineAll(List(Map("a" -> 1), Map("a" -> 1, "b" -> 2))))
    println(List(1, 2, 3).combineAll)
    println(1 |+| 2)
    println("a" |+| "b")

    // Show / Eq / Order
    println(Show[Int].show(3))
    println(Show[String].show("q"))
    println(Show[List[Int]].show(List(1, 2)))
    println(Show[Option[Int]].show(Some(1)))
    println(3.show)
    println(List(1, 2).show)
    println(Eq[Int].eqv(1, 1))
    println(Eq[Int].neqv(1, 2))
    println(Eq[List[Int]].eqv(List(1), List(1)))
    println(Eq[Option[String]].eqv(Some("a"), Some("a")))
    println(1 === 1)
    println(1 =!= 2)
    println(KOrder[Int].compare(1, 2))
    println(KOrder[Int].comparison(2, 2))
    println(KOrder[Int].comparison(3, 2) == Comparison.GreaterThan)
    println(KOrder[String].max("a", "b"))
    println(KOrder[Int].min(4, 2))
    println(List(3, 1, 2).sorted(KOrder[Int].toOrdering))
    println(KOrder[Int].toOrdering.compare(7, 2))

    // SortedSet / NonEmptySet
    val ss: SortedSet[Int] = SortedSet(3, 1, 2)
    println(ss)
    println(Monoid[SortedSet[Int]].combine(ss, SortedSet(4)))
    println(Eq[SortedSet[Int]].eqv(ss, SortedSet(1, 2, 3)))
    println(Foldable[SortedSet].foldMap(ss)(_.toString))
    val nes = cats.data.NonEmptySet.of(3, 1, 2)
    println(nes)
    println(nes.head)
    println(nes.toSortedSet)
    println(nes.map(_ * 2))
    println(nes ++ cats.data.NonEmptySet.one(9))
    println(Semigroup[cats.data.NonEmptySet[Int]].combine(nes, cats.data.NonEmptySet.one(5)))
    println(cats.data.NonEmptySet.fromSet(SortedSet(1, 2)))
    println(cats.data.NonEmptySet.fromSet(SortedSet.empty[Int]))
  }
}
