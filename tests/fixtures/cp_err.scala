// A `match` that runs out of cases throws `scala.MatchError` carrying the
// scrutinee, in both modes. It used to throw a bare
// `RuntimeException("match error")`, which no `catch { case _: MatchError }`
// could see and which said nothing about the value that failed.
object Main {
  sealed trait C
  case class P(v: Int) extends C
  case object Q extends C

  def consOnly(cs: List[C]): Int = cs match {
    case P(v) :: _ => v
  }
  def ints(i: Int): String = i match {
    case 1 => "one"
    case 2 => "two"
  }
  def strs(s: String): String = s match {
    case "a" => "A"
  }

  def cs(c: C): String = c match {
    case P(v) => "p" + v
  }

  def report(body: => Any): Unit =
    try println(body)
    catch { case e: Throwable => println(e.getClass.getName + ": " + e.getMessage) }

  // The private runtime's own `List` has no Scala-style `toString`, so only the
  // thrown class is comparable across the two modes for a list scrutinee.
  def reportClass(body: => Any): Unit =
    try println(body)
    catch { case e: Throwable => println(e.getClass.getName) }

  def main(args: Array[String]): Unit = {
    val qOnly: List[C] = Q :: Nil
    report(consOnly(P(4) :: Nil))
    reportClass(consOnly(qOnly))
    report(ints(1))
    report(ints(3))
    report(strs("a"))
    report(strs("b"))
    report(strs(null))
    report(cs(P(6)))
    report(cs(Q))
  }
}
