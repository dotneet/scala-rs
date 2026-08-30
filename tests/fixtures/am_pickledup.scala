// One pickled declaration reached through two classes is one member.
//
// `IterableOps.map` is not written out by the prelude; it is completed from
// the library pickle on demand and installed on the class that asked. So the
// `Seq` below puts a copy on `scala.collection.immutable.Seq`, the
// `collection.IndexedSeq` puts a second copy on `scala.collection.IndexedSeq`,
// and `scala.IndexedSeq` -- which has both above it and neither below the
// other -- then saw two `map`s that differ only in the vocabulary each copy
// was rewritten into. Every call on it was `ambiguous overload for map`.
//
// The order of the three blocks is the whole point: swap them and the
// duplicate never arises. `map` is only the loudest case, so `flatMap`,
// `filter`, `partition` and `foldLeft` go through the same three receivers.
object Main {
  def main(args: Array[String]): Unit = {
    val s: Seq[Int] = List(1, 2, 3)
    println(s.map(i => i + 1).mkString(","))
    println(s.flatMap(i => List(i, i)).mkString(","))
    println(s.filter(i => i > 1).mkString(","))
    println(s.foldLeft(0)((a, b) => a + b))

    val ci: scala.collection.IndexedSeq[Int] = Vector(4, 5)
    println(ci.map(i => i * 2).mkString(","))
    println(ci.flatMap(i => List(i, -i)).mkString(","))
    println(ci.filter(i => i > 4).mkString(","))
    println(ci.foldLeft(0)((a, b) => a + b))

    val v: IndexedSeq[Int] = Vector(6, 7)
    println(v.map(i => i + 10).mkString(","))
    println(v.map(i => i.toString).mkString("|"))
    println(v.flatMap(i => List(i, i * 100)).mkString(","))
    println(v.filter(i => i > 6).mkString(","))
    println(v.foldLeft(100)((a, b) => a + b))
    val (lo, hi) = v.partition(i => i < 7)
    println(lo.mkString(",") + " / " + hi.mkString(","))
  }
}
