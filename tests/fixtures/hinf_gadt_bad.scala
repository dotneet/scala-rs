// GADT refinement, negatives: a refinement never leaks across cases, a
// class type parameter is never refined (nsc wants a term owner), a
// wildcard case refines nothing, and a contradicting inner refinement is
// discarded (the outer bounds stand).
object Main {
  sealed trait E[T]
  case class I(i: Int) extends E[Int]
  case class B(b: Boolean) extends E[Boolean]
  case class S(s: String) extends E[String]
  def bad1[T](e: E[T]): T = e match {
    case I(i) => i
    case B(b) => 1        // line 12: T = Boolean here
  }
  def bad2[T](e: E[T]): T = e match {
    case _ => 1           // line 15: no refinement
  }
  class C[T](e: E[T]) {
    def get: T = e match {
      case I(i) => i      // line 19: class parameter, not refined
      case _ => ???
    }
  }
  def leak[T](e: E[T], f: E[T]): T = e match {
    case I(i) => f match {
      case S(s) => s      // line 25: T = Int stands, String is not a T
      case _ => i
    }
    case _ => ???
  }
  def wrongVal[T](e: E[T]): T = e match {
    case I(i) => val t: T = "x"; t   // line 31
    case _ => ???
  }
  def main(args: Array[String]): Unit = ()
}
