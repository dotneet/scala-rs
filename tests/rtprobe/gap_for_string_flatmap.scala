// scala-rs rejects: StringOps.flatMap whose function yields Strings
// answers String; yielding a non-Char element answers IndexedSeq.
object Main {
  def main(args: Array[String]): Unit = {
    println(for (c <- "abc"; d <- "xy") yield s"$c$d")
    println("ab".flatMap(c => List(c, c)))
    println("ab".flatMap(c => c.toString * 2))
  }
}
