// User-defined extractors reached with a nested pattern inside them.
//
// `case P(v) ~ t` kept the extractor's `Tuple2` on the stack while the nested
// `P(v)` jumped to the next case, so the two paths disagreed about the stack
// (`VerifyError: Inconsistent stackmap frames`), and `case Some(Two(a, b))` on
// an `Option[Any]` handed `Two.unapply` an erased `Object` with no type test
// in front of it.
object Main {
  sealed trait C
  case class P(v: Int) extends C
  case object Q extends C

  object ~ {
    def unapply(l: List[C]): Option[(C, List[C])] = l match {
      case h :: t => Some((h, t))
      case _ => None
    }
  }

  object Pos {
    def unapply(c: C): Option[Int] = c match {
      case P(v) if v > 0 => Some(v)
      case _ => None
    }
  }

  object Two {
    def unapply(s: String): Option[(Int, String)] = Some((s.length, s))
  }

  def infix(cs: List[C]): String = cs match {
    case P(v) ~ _ => "u" + v
    case Q ~ t => "q" + t.length
    case _ => "-"
  }

  // An extractor nested under `::`: `Pos.unapply` takes a `C`, and the head is
  // read off the cons cell as an `Object`.
  def pos(cs: List[C]): String = cs match {
    case Pos(v) :: _ => "pos" + v
    case _ => "-"
  }

  // The extractor's parameter is narrower than the scrutinee's static type, so
  // the call needs the type test nsc emits in front of it.
  def widened(o: Option[Any]): String = o match {
    case Some(Two(a, b)) => "two" + a + b
    case Some(x) => "other" + x
    case None => "none"
  }

  def main(args: Array[String]): Unit = {
    val pFirst: List[C] = P(4) :: Q :: Nil
    val qFirst: List[C] = Q :: P(4) :: Nil
    val neg: List[C] = P(-1) :: Nil
    println(infix(pFirst))
    println(infix(qFirst))
    println(infix(Nil))
    println(pos(pFirst))
    println(pos(qFirst))
    println(pos(neg))
    println(pos(Nil))
    println(widened(Some("cd")))
    println(widened(Some(3)))
    println(widened(None))
  }
}
