// Count word occurrences in a bare Array[Int] and report the top ones.
object Main {
  val Letters = 26

  def histogram(text: String): Array[Int] = {
    val counts = new Array[Int](Letters)
    var i = 0
    while (i < text.length) {
      val c = text.charAt(i)
      if (c >= 'a' && c <= 'z') counts(c - 'a') += 1
      i += 1
    }
    counts
  }

  def top(counts: Array[Int], n: Int): Array[(Char, Int)] = {
    val pairs = new Array[(Char, Int)](Letters)
    var i = 0
    while (i < Letters) {
      pairs(i) = (('a' + i).toChar, counts(i))
      i += 1
    }
    pairs.filter(_._2 > 0).sortBy(p => (-p._2, p._1)).take(n)
  }

  def main(args: Array[String]): Unit = {
    val counts = histogram("the quick brown fox jumps over the lazy dog the end")
    println(counts.sum)
    println(counts.count(_ > 0))
    val t = top(counts, 4)
    println(t.map { case (c, n) => s"$c=$n" }.mkString(" "))
    println(t.length)
    counts.foreach(_ => ())
    println(counts.indices.filter(i => counts(i) == 0).map(i => ('a' + i).toChar).mkString(""))
  }
}
