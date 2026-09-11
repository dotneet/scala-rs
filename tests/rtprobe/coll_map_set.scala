// Immutable Map and Set operations: updated/+/-, getOrElse, withDefault,
// mapValues, filterKeys, set algebra, SortedMap/TreeSet ordering, and the
// static types those operations answer at.
import scala.collection.immutable.{SortedMap, TreeMap, TreeSet, ListMap}
object Main {
  def main(args: Array[String]): Unit = {
    val m = Map("a" -> 1, "b" -> 2)
    val m2 = m.updated("c", 3)
    val m3 = m + ("d" -> 4) - "a"
    println(m2.toList.sorted + " " + m3.toList.sorted + " " + m.getOrElse("z", 0) + " " + m.get("a") + " " + m.contains("b") + " " + (m2 - "c" == m))
    val md = m.withDefaultValue(-1)
    println(md("a") + " " + md("zz") + " " + m.withDefault(k => k.length)("xyz"))
    println(m.map { case (k, v) => (k * 2, v * 10) }.toList.sorted + " " + m.view.mapValues(_ + 100).toList.sorted + " " + m.view.filterKeys(_ != "a").toList)
    println(m.foldLeft(0)(_ + _._2) + " " + m.values.sum + " " + m.keySet + " " + m.filter(_._2 > 1) + " " + m.count(_._2 > 0))
    println(m ++ Map("a" -> 100) + " " + (Map("a" -> 100) ++ m) + " " + m.updatedWith("a")(_.map(_ + 1)) + " " + m.updatedWith("q")(_ => Some(0)).size)
    val tm = TreeMap(3 -> "c", 1 -> "a", 2 -> "b")
    println(tm + " " + tm.head + " " + tm.last + " " + tm.range(1, 3) + " " + (tm + (0 -> "z")).keys.toList)
    val sm: SortedMap[String, Int] = SortedMap("b" -> 2, "a" -> 1)
    println(sm.updated("c", 3) + " " + sm.map { case (k, v) => (v, k) })
    val lm = ListMap("z" -> 1, "y" -> 2, "x" -> 3)
    println(lm + " " + lm.updated("a", 0) + " " + (lm - "y"))
    val s = Set(1, 2, 3); val t = Set(3, 4)
    println((s | t).toList.sorted + " " + (s & t) + " " + (s &~ t).toList.sorted + " " + s.subsetOf(Set(1, 2, 3, 4)) + " " + (s + 3).size + " " + (s - 1).toList.sorted)
    println(s.map(_ % 2) + " " + s.filter(_ > 1).toList.sorted + " " + s.contains(2) + " " + s(5) + " " + TreeSet(5, 1, 3) + " " + TreeSet("b", "a").toList)
    println(TreeSet(1, 2, 3).map(-_) + " " + TreeSet(1, 5, 9).rangeFrom(4) + " " + TreeSet(4, 2).min + " " + s.max)
    val counts = "mississippi".groupBy(identity).view.mapValues(_.length).toMap
    println(counts.toList.sorted)
    val inverted = m.map(_.swap)
    println(inverted(2) + " " + inverted.getClass.getSimpleName.startsWith("Map"))
    val mm = (1 to 6).map(i => i -> i * i).toMap
    println(mm.size + " " + mm(5) + " " + mm.keys.toList.sorted + " " + mm.maxBy(_._2))
    val big = (1 to 40).map(i => (i, i.toString)).toMap
    println(big.size + " " + big(33) + " " + big.filter(_._1 > 38).toList.sorted)
    println(Map.empty[Int, Int].isEmpty + " " + Set.empty[String].size + " " + m.isEmpty + " " + m.nonEmpty + " " + m.head._1.nonEmpty)
    println(m.toSeq.sortBy(-_._2) + " " + m.partition(_._2 > 1)._1 + " " + m.find(_._2 == 2) + " " + m.exists(_._1 == "a"))
    println(m.transform((k, v) => k + v).toList.sorted + " " + m.collect { case (k, v) if v > 1 => k })
    val zipped = List("p", "q").zip(List(1, 2)).toMap
    println(zipped + " " + zipped.getOrElse("r", "none"))
  }
}
