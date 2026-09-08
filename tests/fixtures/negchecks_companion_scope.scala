// A companion reading `private` state the way Scala does allow.

class Counter(private val start: Int) {
  private def step = 2
  private[this] val tag = "c"
  def show = tag + start
}

object Counter {
  // The companion of a top-level class sees its `private` members, on any
  // instance -- this is the rule `t8002-nested-scope` does *not* break.
  def twice(c: Counter): Int = c.start + c.step * 2
}

object Main {
  def local: Int = {
    // A local class and a local object in the *same* block are companions,
    // and the object may read the class's `private` member on another
    // instance. `t8002-nested-scope` is this program with the object moved
    // into a nested block.
    class L { private def hidden = 7 }
    object L { def read = new L().hidden }
    L.read
  }

  def nestedButNotCompanions: Int = {
    // The same two names in different blocks are *not* companions, so `M`
    // here reads its own member and nothing of the class's.
    class M { def open = 3 }

    {
      val a = 1
      object M { def read = new M().open }
      M.read + a
    }
  }

  def main(args: Array[String]): Unit = {
    println(Counter.twice(new Counter(5)))
    println(new Counter(5).show)
    println(local)
    println(nestedButNotCompanions)
  }
}
