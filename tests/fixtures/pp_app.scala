// Compiled by real scalac against `pp_lib`'s class files. Every call site
// here writes the parentheses the declaration wrote; before the pickle
// carried the empty parameter list, scalac answered "Empty does not take
// parameters" to the first two lines of `main` alone.
object Main {
  def main(args: Array[String]): Unit = {
    println(Keys.empty())
    println(Empty())
    println(Empty.apply())
    println(Empty().copy())
    println(Keys.nullary)
    Keys.unit()
    println(Keys.withImplicit())
    println(Keys.curried(1)(2))
    println(Keys.emptyThenValue()(4))
    println(Keys.generic("x")())
    println(Keys.deep()()())
    println(Pair(1, 2).copy(b = 5))
    val c = new Counter(0)
    println(c.tick())
    println(c.twice())
    println(c.label)
    val l = new Loud(0)
    println(l.tick())
    val t: Ticker = l
    println(t.tick())
  }
}
