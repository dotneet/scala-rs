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
    // A `HashMap` receiver reaches `getOrElse` through both the prelude's
    // `mutable.Map` and the pickled `collection.MapOps`; the call has to stay
    // one member, and has to run.
    val hm2 = mutable.HashMap.empty[String, Sub]
    hm2.update("k", new Sub)
    println(hm2.getOrElse("k", new Base).toString + " " + hm2.getOrElse("x", new Base))

    // `Option` as a collection.
    val some: Option[String] = Some("x")
    val none: Option[String] = None
    println((Seq("a") ++ some).mkString(","))
    println((Seq("a") ++ none).mkString(","))
    val it: Iterable[String] = some
    println(it.size)

    // `new StringBuilder(initCapacity, initValue)`.
    val sb = new StringBuilder(8, "ab")
    sb.append("c")
    println(sb.toString)
  }
}
