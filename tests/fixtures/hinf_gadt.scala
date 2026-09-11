// GADT-style refinement of a method type parameter inside a case (nsc's
// inferTypedPattern / inferConstructorInstance bounds): a typed pattern or
// a case-class pattern whose class fixes the scrutinee's type argument
// makes that argument known for the case.
object Main {
  sealed trait E[T]
  case class I(i: Int) extends E[Int]
  case class S(s: String) extends E[String]
  case class B(b: Boolean) extends E[Boolean]
  case class Add(l: E[Int], r: E[Int]) extends E[Int]
  case class P[A, B](a: E[A], b: E[B]) extends E[(A, B)]
  case class Box[A](a: A) extends E[A]
  sealed trait Co[+T]
  case class CI(i: Int) extends Co[Int]
  case class CS(s: String) extends Co[String]

  // scala/util/Sorting.scala's `sort[T]`: `ord: Ordering[T]` is an
  // `Ordering[Int]` inside `case a: Array[Int]`.
  def ms[T](a: Array[T], ord: Ordering[T]): String = a.sorted(ord).mkString(",")
  def sort[T](a: Array[T], ord: Ordering[T]): String = (a: Array[T]) match {
    case a: Array[Int] => "int:" + ms[Int](a, ord)
    case a: Array[Double] => "double:" + ms[Double](a, ord)
    case _ => "other:" + ms(a, ord)
  }
  def ev[T](e: E[T]): T = e match {
    case I(i) => i
    case S(s) => s
    case B(b) => b
    case Add(l, r) => ev(l) + ev(r)
    case P(a, b) => (ev(a), ev(b))
    case Box(a) => a
  }
  def ev2[T](e: E[T]): T = e match {
    case x: I => x.i
    case x: B => x.b
    case x: Add => ev2(x.l) + ev2(x.r)
    case other => ev(other)
  }
  // Covariant: only a lower bound (`T >: Int`), enough for the body.
  def co[T](c: Co[T]): T = c match {
    case CI(i) => i
    case CS(s) => s
  }
  // The refinement reaches other values of type T, including member
  // selection on them (`t + 1` selects `Int.+`; the receiver is unboxed).
  def other[T](e: E[T], t: T): String = e match {
    case I(_) => (t + 1).toString
    case S(_) => t.toUpperCase
    case _ => "?"
  }
  // Nested matches refine further and restore on the way out.
  def nested[T](e: E[T], f: E[T]): T = e match {
    case I(i) => f match {
      case I(j) => i + j
      case _ => i
    }
    case S(s) => s
    case _ => ev(e)
  }
  // A local of type T inside the refined case (erasure stays Object).
  def local[T](e: E[T]): T = e match {
    case I(i) => val t: T = i; t
    case S(s) => val t: T = s; t
    case _ => ev(e)
  }
  // A declared primitive upper bound on a receiver (same unboxing).
  def bounded[T <: Int](t: T): Int = t + 1

  def main(args: Array[String]): Unit = {
    println(sort(Array(3, 1, 2), Ordering.Int))
    println(sort(Array(2.5, 1.5), Ordering.Double.TotalOrdering))
    println(sort(Array("b", "a"), Ordering.String))
    println(ev(Add(I(1), I(2))))
    println(ev(P(I(1), S("a"))))
    println(ev(Box(2.5)))
    println(ev(B(true)))
    println(ev2(Add(I(1), I(2))))
    println(co(CI(3)))
    println(co(CS("c")))
    println(other(I(1), 41))
    println(other(S(""), "up"))
    println(nested(I(1), I(2)))
    println(nested(S("s"), S("t")))
    println(local(I(7)))
    println(local(S("l")))
    println(bounded(41))
  }
}
