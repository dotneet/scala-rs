// Two methods of the same object each declaring `case class P(...)`: each
// gets its own class *and* its own companion module class, indexed
// separately (`Main$P$1` / `Main$P$1$` vs `Main$P$2` / `Main$P$2$` under our
// naming, since the companion's jvm name is derived from the case class's
// own already-indexed name). Neither companion may leak into the other
// method.
object Main {
  def m1(): String = {
    case class P(n: Int)
    P(1).toString
  }
  def m2(): String = {
    case class P(s: String)
    P("x").toString
  }
  def main(a: Array[String]): Unit = {
    println(m1())
    println(m2())
  }
}
