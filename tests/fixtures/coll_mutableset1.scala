import scala.collection.mutable

object Main {
  def main(args: Array[String]): Unit = {
    val s = mutable.Set[Int]()
    s += 1
    s += 2
    s += 3
    println(s.size)
    println(s.isEmpty)
    println(s.nonEmpty)
    println(s.contains(2))
    println(s.contains(99))
    s -= 2
    println(s.contains(2))
    println(s.size)
    println(s.toList.length)
    println(s.toSeq.length)
    val it = s.iterator
    println(it.hasNext)
    var sum = 0
    s.foreach(x => sum += x)
    println(sum)
    val doubled = s.map(x => x * 10)
    println(doubled.size)
    val big = s.filter(x => x > 1)
    println(big.size)
    println(s.mkString(","))
    println(s.mkString("[", ",", "]"))
    s.clear()
    println(s.isEmpty)
  }
}
