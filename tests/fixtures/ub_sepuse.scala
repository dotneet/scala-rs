// Uses `ub_sepdef.scala`'s `Unit` members through `-cp`.
object Main {
  def main(args: Array[String]): Unit = {
    println(Lib.f(()))
    println(Lib.middle(1, (), "s"))
    println(LK((), 2))
    println(LK((), 2).k)
    val c = new LC(())
    println(c.u)
    println(c.w)
    // No `c.w = ()`: a `var` read back through `-cp` is not recognised as
    // mutable yet, for every field type (`reassignment to val w`).
    println(c.m(()))
    LK((), 2) match {
      case LK(u, n) => println(u); println(n)
    }
  }
}
