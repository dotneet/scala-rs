// Three slick roots that all surfaced as `no matching overload`:
//   * `mutable.HashSet` / `mutable.HashMap` are a `collection.Set` / `Map`,
//   * `Map.getOrElse[V1 >: V]` widens on its default,
//   * `Option.option2Iterable` makes an `Option` an `IterableOnce`.
import scala.collection.mutable

object Main {
  class Base { override def toString = "Base" }
  class Sub extends Base { override def toString = "Sub" }

  def countSet(s: scala.collection.Set[String]): Int = s.size
  def countMap(m: scala.collection.Map[String, Int]): Int = m.size

  def main(args: Array[String]): Unit = {
    val hs = mutable.HashSet.empty[String]
    hs += "a"
    hs += "b"
    println(countSet(hs))

    val hm = mutable.HashMap.empty[String, Int]
    hm.update("a", 1)
    println(countMap(hm))

    // `getOrElse[V1 >: V]` on both Map flavours.
    val im: Map[String, Sub] = Map("k" -> new Sub)
    val d1: Base = im.getOrElse("absent", new Base)
    val d2: Base = im.getOrElse("k", new Base)
    println(d1.toString + " " + d2.toString)
    val mm = mutable.Map[String, Sub]("k" -> new Sub)
    val d3: Base = mm.getOrElse("absent", new Base)
    println(d3)

    // `Option` as a collection.
    val some: Option[String] = Some("x")
    val none: Option[String] = None
    println((Seq("a") ++ some).mkString(","))
    println((Seq("a") ++ none).mkString(","))
    val it: Iterable[String] = some
    println(it.size)
  }
}
