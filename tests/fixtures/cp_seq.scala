// Sequence patterns with a nested constructor pattern inside them, and an
// extractor whose `Option` holds a tuple wider than `Tuple2` -- the binder
// used to `checkcast scala/Tuple2` whatever the arity was.
//
// `Seq(...)` / `List(...)` extractor patterns and `Tuple3` only exist against
// the real scala-library, so this fixture is jar-only.
object Main {
  sealed trait C
  case class P(v: Int) extends C
  case object Q extends C

  object Tri {
    def unapply(s: String): Option[(Int, String, Int)] = Some((s.length, s, s.length * 2))
  }

  def listPat(cs: List[C]): String = cs match {
    case List(P(a), Q) => "L" + a
    case List(Q, P(a)) => "l" + a
    case _ => "-"
  }

  def seqPat(cs: Seq[C]): String = cs match {
    case Seq(P(a), _*) => "S" + a
    case Seq(Q, rest @ _*) => "s" + rest.length
    case _ => "-"
  }

  def tri(s: String): String = s match {
    case Tri(a, b, c) => "" + a + b + c
    case _ => "-"
  }

  def main(args: Array[String]): Unit = {
    val pq: List[C] = List(P(1), Q)
    val qp: List[C] = List(Q, P(2))
    println(listPat(pq))
    println(listPat(qp))
    println(listPat(Nil))
    println(seqPat(pq))
    println(seqPat(qp))
    println(seqPat(Nil))
    println(tri("ab"))
  }
}
