object Main {
  import scala.collection.immutable.SortedMap
  def main(a: Array[String]): Unit = {
    val m = SortedMap(3 -> "c", 1 -> "a", 2 -> "b")
    println(m)
    println(m.head, m.last)
    println(m.range(1, 3))
    println(m.to(2), m.from(2))
    val s = scala.collection.immutable.SortedSet(5, 1, 3)
    println(s, s.min, s.max)
    println(s.rangeFrom(2))
    println(m.firstKey, m.lastKey)
    println(m + (0 -> "z"))
  }
}
