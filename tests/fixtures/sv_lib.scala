// The same three fixes against members only the real scala-library backs:
// `Array[List[Int]]` (element erasure), `Int.max` as an alphabetic infix
// operator on the right of an op-assignment, `++=` on a `var List`, and a
// `foreach` lambda whose body ends in a definition.
object Main {
  def main(args: Array[String]): Unit = {
    val l = new Array[List[Int]](2)
    l(0) = List(1, 2)
    l(1) = Nil
    println(l(0))
    println(l.getClass.getName)

    var n = 0
    val i = 1
    val x = 2
    n += i max x
    println(n)
    n += i min x
    println(n)

    var lst = List(1)
    lst ++= List(2, 3)
    println(lst)

    var seen = 0
    List(1, 2, 3).foreach { q => val y = q; seen += y }
    println(seen)

    // A lambda body that is only a definition still has value `()`.
    List(1, 2).foreach { q => val y = q }
    println("done")
  }
}
