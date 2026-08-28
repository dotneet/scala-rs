import scala.collection.mutable

object Main {
  def main(args: Array[String]): Unit = {
    val b = mutable.ArrayBuffer[Int]()
    for (i <- 1 to 5) b += i * i
    println(b.mkString(" "))
    println(b.mkString(","))
    println(b.mkString("[", ",", "]"))
    println(b.length)
    println(b.size)
    println(b.isEmpty)
    println(b.nonEmpty)
    println(b.head)
    println(b.last)
    b.foreach(x => print(x + " "))
    println()
    val m2 = b.map(x => x + 1)
    println(m2.mkString(" "))
    val f2 = b.filter(x => x > 5)
    println(f2.mkString(" "))
    println(b.toList)
    val it = b.iterator
    println(it.hasNext)
    println(b.contains(9))
    println(b.indexOf(9))
    println(b.reverse.mkString(" "))
    println(b.foldLeft(0)((acc, x) => acc + x))
    b.append(100)
    println(b.mkString(" "))
    b ++= List(1, 2, 3)
    println(b.mkString(" "))
    b -= 100
    println(b.mkString(" "))
    b.insert(0, -1)
    println(b.mkString(" "))
    val removed = b.remove(0)
    println(removed)
    println(b.sortBy(x => -x).mkString(" "))
    println(b.sorted.mkString(" "))
    b.clear()
    println(b.isEmpty)
  }
}
