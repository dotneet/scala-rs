// With no explicit argument the parameter *is* pinned at its lower bound, so
// the search is for the witness of that bound and its absence is an error --
// not an unapplied method type standing as the value.
object Main {
  val ss: List[String] = List("a")
  val a = ss.sum
  val b = ss.toVector.product
}
