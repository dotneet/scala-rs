// A plain class's constructor parameter without `val` is private state, not a
// member: only a `case class`'s first parameter list becomes accessors on its
// own. Reading one from outside is an error, not a field access we let
// through.

class Plain(hidden: Int) {
  def twice: Int = hidden * 2
}

object Main {
  def main(args: Array[String]): Unit = {
    val p = new Plain(3)
    println(p.twice)
    println(p.hidden)
  }
}
