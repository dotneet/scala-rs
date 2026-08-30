// A nested pattern inside `::` is still checked: the head sub-pattern has to
// name a real extractor, with the arity that extractor has.
object Main {
  sealed trait C
  case class P(v: Int) extends C

  def wrongArity(cs: List[C]): Int = cs match {
    case P(a, b) :: _ => a + b
    case _ => 0
  }

  def noSuchExtractor(cs: List[C]): Int = cs match {
    case Nope(a) :: _ => a
    case _ => 0
  }
}
