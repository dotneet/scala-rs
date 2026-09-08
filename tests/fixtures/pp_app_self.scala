// The same library half, read back by scala-rs itself off `-cp`.
//
// Only the empty-parameter-list cases: a *curried* method still reaches our
// own classpath reader as one joined list (`crates/backend/src/pickle.rs`'s
// subset decoder models a member as one flat parameter list), so
// `Keys.curried(1)(2)` is a call our own compiler does not accept against its
// own class files -- unchanged by this fix, and covered against real scalac by
// `pp_app.scala`.
object Main {
  def main(args: Array[String]): Unit = {
    println(Keys.empty())
    println(Empty())
    println(Empty.apply())
    println(Keys.nullary)
    Keys.unit()
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
