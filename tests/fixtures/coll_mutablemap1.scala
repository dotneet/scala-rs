import scala.collection.mutable

object Main {
  def main(args: Array[String]): Unit = {
    val m = mutable.Map[String, Int]()
    m("a") = 1
    m.update("b", 2)
    m += ("c" -> 3)
    println(m.size)
    println(m.isEmpty)
    println(m.nonEmpty)
    println(m("a"))
    println(m.get("z"))
    println(m.getOrElse("z", -1))
    println(m.getOrElseUpdate("d", 4))
    println(m("d"))
    println(m.contains("a"))
    println(m.contains("zzz"))
    val keys = m.keys.toList
    println(keys.length)
    val values = m.values.toList
    println(values.length)
    m -= "d"
    println(m.contains("d"))
    val removed = m.remove("c")
    println(removed)
    println(m.toList.length)
    println(m.toSeq.length)
    println(m.mkString(","))
    var sum = 0
    m.foreach(p => sum += p._2)
    println(sum)
    val doubled = m.filter(p => p._2 > 1)
    println(doubled.size)
    m.clear()
    println(m.isEmpty)
  }
}
