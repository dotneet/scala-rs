// The reader half of the separate-compilation ABI check; see `nx_lib.scala`.
// Whichever compiler builds this file has to reach every member of the other
// one's class files the way that compiler published them.
import nxlib.{Holder, NullSig, Store, Sub}

object Main {
  def main(args: Array[String]): Unit = {
    val h = new Holder(3, 9)
    println(h.n)
    println(h.q)
    h.c = 5
    println(h.c)
    println(h.bump)
    val s = new Sub
    println(s.n)
    println(s.q)
    println(Store.greeting)
    Store.count = 4
    println(Store.count)
    val ns = new NullSig
    println(ns.n)
    println(ns.take(null))
    // `isEmpty` rather than the list itself: the private runtime's `Nil` has
    // no `toString` of its own, and this file is also compiled in that mode.
    println(ns.ln.isEmpty)
  }
}
