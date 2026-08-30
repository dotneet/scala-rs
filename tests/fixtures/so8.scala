object Main {
  def main(args: Array[String]): Unit = {
    val s = "abcdef"

    // Reached through `Predef.wrapString` (WrappedString), not StringOps.
    println(s.zipWithIndex)
    println(s.zip(List(1, 2, 3)))
    println(s.scanLeft(0)((a: Int, c: Char) => a + c.toInt))

    // StringOps proper.
    println(s.sliding(2).toList)
    println(s.sliding(3, 2).toList)
    println(s.groupBy((c: Char) => c).toList.sortBy((p: (Char, String)) => p._1))
    println(s.sortBy((c: Char) => -c.toInt))
    println(s.sortWith((a: Char, b: Char) => a > b))
    println(s.sorted)
    println(s.distinctBy((c: Char) => c))

    // The result-type overload pair: a `Char`-valued case block gives a
    // `String`, anything else an `IndexedSeq`.
    println(s.collect { case c if c > 'c' => c })
    println(s.collect { case c if c > 'c' => c.toInt })

    println(s.partition((c: Char) => c > 'c'))
    println(s.span((c: Char) => c < 'c'))
    println(s.splitAt(2))
    println(s.tails.toList)
    println(s.inits.toList)
    println(s.permutations.take(3).toList)
    println(s.combinations(2).take(3).toList)
    println(s.indexWhere((c: Char) => c > 'c'))
    println(s.indexWhere((c: Char) => c > 'c', 4))
    println(s.lastIndexWhere((c: Char) => c > 'c'))
    println(s.lastIndexWhere((c: Char) => c > 'c', 3))
    println(s.fold('a')((a: Char, b: Char) => if (a > b) a else b))
    println(s.prepended('x'))
    println(s.appended('x'))
    println(s :++ "xy")
    println("xy" ++: s)
    println(s.linesWithSeparators.toList)
    println(s.view.take(3).toList)

    // `apply`, both spellings.
    println(s.apply(1))
    println(s(4))

    // `withFilter` returns a `StringOps.WithFilter`, whose own `map` is the
    // same String/IndexedSeq pair.
    println(s.withFilter((c: Char) => c > 'c').map((c: Char) => c.toUpper))
    println(s.withFilter((c: Char) => c > 'c').map((c: Char) => c.toInt))
    s.withFilter((c: Char) => c > 'd').foreach((c: Char) => println(c))

    val sb = new StringBuilder
    s.addString(sb)
    println(sb.toString)
    val sb2 = new StringBuilder
    s.addString(sb2, "-")
    println(sb2.toString)
    val sb3 = new StringBuilder
    s.addString(sb3, "[", "-", "]")
    println(sb3.toString)
  }
}
