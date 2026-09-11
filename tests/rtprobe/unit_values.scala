// Unit-typed expressions used as values: if without else, assignments,
// while, blocks ending in definitions, Unit in collections and generics,
// and value discarding when a non-Unit expression meets an expected Unit.
object Main {
  var x = 0
  def sideEffect(): Int = { x += 1; x }
  def discard(): Unit = sideEffect()
  def unitParam(u: Unit): String = "got " + u
  def main(args: Array[String]): Unit = {
    val a = if (x > 100) 1
    println(a)
    val b = (x = 5)
    println(b + " " + x)
    val c = while (x < 7) x += 1
    println(c + " " + x)
    val d = { val y = 1 }
    println(d)
    val e: Unit = 42
    println(e)
    discard(); println(x)
    val f: Unit = sideEffect()
    println(f + " " + x)
    println(unitParam(()) + " " + x)
    val lst = List((), println("in list"), ())
    println(lst.size + " " + lst)
    val g = List(1, 2).foreach(_ + 1)
    println(g)
    val h: Any = ()
    println(h == () + " " + (h == ()) + " " + h.isInstanceOf[Unit] + " " + h.getClass.getSimpleName)
    val opt: Option[Unit] = Some(())
    println(opt + " " + opt.get + " " + opt.map(_ => 1))
    val fnU: Int => Unit = i => x += i
    fnU(10); println(x)
    val m = Map(1 -> ())
    println(m)
    def returnsUnit(): Unit = return
    println(returnsUnit())
    val unitArr = new Array[Unit](2)
    println(unitArr.toList)
    val t = ((), 1)
    println(t._1 + " " + t)
    val u1 = (); val u2 = ()
    println((u1 == u2) + " " + u1.hashCode + " " + u1.toString)
  }
}
