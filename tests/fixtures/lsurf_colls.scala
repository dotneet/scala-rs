// Library surface: what a collection transformation really returns (the
// receiver's pickled `IterableOps[A, CC, C]`), `SortedSet.map` through its
// `Ordering` overload, `map` on `Option` / `Try` / `Either` with a real type
// parameter, `Try[T](…)`, and `TupleN <: ProductN`.
import scala.collection.immutable.{BitSet, NumericRange, SortedSet}
import scala.collection.IndexedSeqView
import scala.util.{Success, Try}

class MySeq[A](val xs: Vector[A]) extends scala.collection.immutable.Seq[A] {
  def apply(i: Int): A = xs(i)
  def length: Int = xs.length
  def iterator: Iterator[A] = xs.iterator
}

object Main {
  def t(name: String)(f: => Any): Unit =
    try println(name + ": " + f)
    catch { case e: Throwable => println(name + ": " + e.getClass.getName) }

  def sortedMap(s: SortedSet[Int]) = s.map(_ + 1)

  def main(args: Array[String]): Unit = {
    val v: IndexedSeqView[Int] = Vector(1, 2, 3, 4).view
    val nr: NumericRange[Long] = 1L to 5L
    val s = SortedSet(3, 1)
    val my = new MySeq(Vector(1, 2, 3))
    t("view filter") { v.filter(_ % 2 == 0).toList }
    t("view collect") { v.collect { case x if x > 2 => x }.toList }
    t("view map") { v.map(_ + 1).toList }
    t("range map") { nr.map(_ * 2) }
    t("range filter") { nr.filter(_ > 2) }
    t("range take") { nr.take(2) }
    t("chars map") { ('a' to 'c').map(_.toUpper) }
    t("sorted map") { s.map(_ * 2) }
    t("sorted map str") { s.map(_.toString + "!") }
    t("sorted flatMap") { s.flatMap(x => List(x, x * 10)) }
    t("sorted collect") { s.collect { case x if x > 1 => x * 2 } }
    t("sorted filter") { s.filter(_ > 1) }
    t("bitset map") { BitSet(1, 2).map(_ + 1) }
    t("bitset map str") { BitSet(1, 2).map(_.toString) }
    t("user seq collect") { my.collect { case x if x > 1 => x * 10 } }
    t("user seq map") { my.map(_ + 1) }
    val m = scala.collection.mutable.LinkedHashMap("one" -> 1, "two" -> 2, "three" -> 3)
    t("mapview drop") { m.view.mapValues(-_).drop(1).map(_._1).toList }
    t("option map") { val o: Option[String] = Option(3).map(_.toString); o }
    t("try map") { val x: Try[Int] = Try("12").map(_.toInt); x }
    t("either map") { val e: Either[String, Long] = (Right(3): Either[String, Int]).map(_.toLong); e }
    t("try explicit") { Try[Int](throw new RuntimeException("boom")).map(_ + 1).isFailure }
    t("for option") { for (x <- Option(1); y <- Option(2)) yield x + y }
    t("for try") { for (x <- Try(1); y <- Success(2)) yield x * y }
    val p: Product2[Int, String] = (1, "a")
    t("tuple as product") { p._2 }
    val op: Option[Product2[Int, Int]] = Some((4, 5))
    t("option product") { op.map(_._1) }
  }
}
