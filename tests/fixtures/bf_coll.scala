// 2.13 `BuildFrom`: a transformation's result is the *receiver's* collection,
// not the class the inherited declaration happened to name.
//
// Library-ABI only: every result type below is a real `scala.collection`
// class, and the private runtime has no `MapOps`, `Factory` or `TreeMap` to
// back them.
import scala.collection.immutable.{SortedMap, TreeMap, TreeSet}
import scala.collection.mutable.{ArrayBuffer, ListBuffer}

case class E(d: String, s: Int)

object Main {
  def main(args: Array[String]): Unit = {
    // MapOps.map[K2, V2](f: ((K, V)) => (K2, V2)): CC[K2, V2]
    val m: Map[String, List[Int]] = Map("x" -> List(1, 2))
    println(m.map { case (d, g) => d -> g.sum })
    val n: Map[String, Int] = m.map { case (d, g) => d -> g.sum }
    println(n)
    // A lambda that does not return a pair keeps `Iterable[B]`, as in nsc.
    println(m.map { case (_, g) => g.sum }.toList)

    val m2: Map[String, Int] = Map("a" -> 1, "b" -> 2)
    val f2: Map[String, Int] = m2.filterNot(_._2 > 1)
    println(f2)
    val c2: Map[String, Int] = m2.collect { case (k, v) if v > 1 => (k, v * 10) }
    println(c2)
    val fm: Map[String, Int] = m2.flatMap { case (k, v) => List((k, v + 1)) }
    println(fm.toList.sorted)
    val cc: Map[String, Int] = m2 ++ Map("c" -> 3)
    println(cc.toList.sorted)
    val tk: Map[String, Int] = m2.take(1)
    println(tk.size)
    val pt: (Map[String, Int], Map[String, Int]) = m2.partition(_._2 > 1)
    println(pt._1.size + "/" + pt._2.size)
    val gb: Map[String, Map[String, Int]] = m2.groupBy(_._1)
    println(gb("a").toList)

    // groupMapReduce / groupMap: the third clause is inferred from the second.
    val es = List(E("x", 10), E("y", 20), E("x", 5))
    println(es.groupMapReduce(_.d)(_.s)(_ + _).toList.sorted)
    println(es.groupMap(_.d)(_.s).toList.map(p => (p._1, p._2.sum)).sorted)
    println(es.groupBy(_.d).toList.map(p => (p._1, p._2.size)).sorted)

    // Sorted maps keep their own class through `-`, `+`, `updated`, `map`.
    val t = TreeMap(1L -> "a", 2L -> "b")
    val t1: TreeMap[Long, String] = t - 1L
    println(t1)
    val t2: TreeMap[Long, String] = t + ((3L, "c"))
    println(t2)
    val t3: TreeMap[Long, String] = t.updated(4L, "d")
    println(t3)
    val sm: SortedMap[String, Int] = SortedMap("b" -> 2) ++ Map("a" -> 1)
    println(sm)

    // Sets and sorted sets.
    val s1: Set[Int] = Set(1, 2, 3) ++ List(9)
    println(s1.toList.sorted)
    val ts: TreeSet[Int] = TreeSet(3, 1, 2) - 1
    println(ts)

    // IndexedSeq / LazyList do not fall back to `Seq`.
    val ix: IndexedSeq[Int] = Vector(1, 2, 3)
    val ix2: IndexedSeq[Int] = ix.flatMap(i => List(i, i))
    println(ix2)
    val ix3: IndexedSeq[(Int, String)] = ix.zip(List("a", "b", "c"))
    println(ix3)
    val ix4: (IndexedSeq[Int], IndexedSeq[Int]) = ix.partition(_ > 1)
    println(ix4._1 + " " + ix4._2)
    val ix5: Map[Boolean, IndexedSeq[String]] = ix.groupMap(_ > 1)(_.toString)
    println(ix5(true))

    // `to(factory)`: IterableFactory.toFactory / MapFactory.toFactory.
    val ab: ArrayBuffer[Int] = List(1, 2, 3).to(ArrayBuffer)
    println(ab)
    val lb: ListBuffer[Int] = Vector(1, 2).to(ListBuffer)
    println(lb)
    val bl: List[Int] = Set(7).to(List)
    println(bl)
    val bm: Map[String, Int] = List(("k", 1)).to(Map)
    println(bm)
    // The same evidence, found as an implicit *value*.
    val fac = implicitly[scala.collection.Factory[Int, Vector[Int]]]
    println(fac.fromSpecific(List(4, 5)))

    // mutable.Map keeps its own class through `-`.
    val mm = scala.collection.mutable.Map("a" -> 1, "b" -> 2)
    val mm1: scala.collection.mutable.Map[String, Int] = mm - "a"
    println(mm1)
  }
}
