// `StringOps.map` is two overloads in 2.13, told apart by nothing but the
// function's result type: `map(Char => Char): String` and
// `map[B](Char => B): IndexedSeq[B]`.
object Main {
  def main(args: Array[String]): Unit = {
    println("ab".map((c: Char) => if (c == 'a') 'A' else c))
    println("abc".map(c => c.toUpper))
    println("abc".map(c => c.toString))
    println("abc".map(c => c.toInt))
    println("ab".map(_.toString).mkString("-"))
    val s: String = "abc".map(c => c)
    println(s)
    val n: IndexedSeq[Int] = "abc".map(c => c.toInt)
    println(n.size)
    // The `Char => Char` alternative must not be picked just because it is
    // the more specific one when the literal's result is not a `Char`.
    println("hello".map(c => c == 'l').mkString(","))
  }
}
