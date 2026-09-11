// Library surface: explicit type arguments on a zero-argument Java static
// method followed by `()`, and `StringContext(..).s(..)` written out as a
// call (a macro in 2.13; nsc expands it to `standardInterpolator`).
object Main {
  def main(args: Array[String]): Unit = {
    val it = java.util.Collections.emptyIterator[String]()
    println(it.hasNext)
    val l = java.util.Collections.emptyList[Int]()
    println(l.size)
    val o = java.util.Optional.empty[String]()
    println(o.isPresent)
    println(java.util.Collections.singletonList[String]("a").get(0))
    val sc = StringContext("a", "b", "c")
    println(sc.s(1, 2))
    println(StringContext("x\\ty").s())
    val parts = Seq(1, 2)
    println(sc.s(parts: _*))
    try println(StringContext("p1", "p2", "p3").s("e1"))
    catch { case ex: IllegalArgumentException => println(ex.getMessage) }
  }
}
