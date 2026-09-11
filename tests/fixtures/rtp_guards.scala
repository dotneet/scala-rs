// Every guard of a for-comprehension generator filters. The parser kept only
// the last `if` of `x <- e if a; if b` (and of `if a if b`), so the earlier
// filters were silently dropped.
object Main {
  def main(args: Array[String]): Unit = {
    println(for (i <- 1 to 10 if i % 2 == 0; if i > 4) yield i)
    println(for (i <- List(1, 2, 3, 4, 5, 6) if i > 1 if i < 5) yield i)
    for (i <- 1 to 6 if i % 2 == 0; if i > 2) print(i)
    println()
    val log = new StringBuilder
    val r = for {
      i <- List(1, 2, 3, 4)
      if { log.append(s"a$i "); i > 1 }
      if { log.append(s"b$i "); i < 4 }
      j = i * 10
      if j != 20
    } yield j
    println(r + " " + log.toString.trim)
    println(for ((k, v) <- Map(1 -> "a", 2 -> "b", 3 -> "c") if k > 1; if v != "c") yield k)
  }
}
