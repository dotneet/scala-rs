// Nested patterns under `::`. The head of a cons cell is read as an erased
// `Object` and used to be cast straight to the sub-pattern's type, so
// `case P(v) :: t` threw a `ClassCastException` on every list whose head was
// not a `P` instead of falling through to the next case.
object Main {
  sealed trait C
  case class P(v: Int) extends C
  case class R(s: String) extends C
  case object Q extends C

  def sum(cs: List[C]): Int = cs match {
    case Nil => 0
    case P(v) :: t => v + sum(t)
    case R(s) :: t => s.length + sum(t)
    case Q :: t => sum(t)
  }

  // Depth two on the left, and a stable id one level down.
  def pairs(cs: List[C]): String = cs match {
    case P(a) :: P(b) :: _ => "PP" + a + b
    case P(a) :: Q :: _ => "PQ" + a
    case _ => "-"
  }

  // An extractor to the right of `::`.
  def second(cs: List[C]): String = cs match {
    case _ :: P(v) :: _ => "2nd" + v
    case _ => "-"
  }

  def guarded(cs: List[C]): String = cs match {
    case P(v) :: _ if v > 1 => "big" + v
    case P(v) :: _ => "small" + v
    case _ => "-"
  }

  // `p @ P(v)` binds at the pattern's type, after the pattern's own test.
  def bound(cs: List[C]): String = cs match {
    case (p @ P(v)) :: _ => "b" + p.v + v
    case _ => "-"
  }

  def typed(cs: List[C]): String = cs match {
    case (p: P) :: _ => "t" + p.v
    case _ => "-"
  }

  // A tuple sub-pattern is a constructor pattern too.
  def tup(ps: List[(Int, String)]): String = ps match {
    case (a, b) :: _ => "" + a + b
    case _ => "-"
  }

  // The same shape one extractor deeper.
  def nested(o: Option[C]): String = o match {
    case Some(P(v)) => "sp" + v
    case Some(Q) => "sq"
    case Some(_) => "so"
    case None => "none"
  }

  // A constant sub-pattern compares; it must not unbox first.
  def anyLit(o: Option[Any]): String = o match {
    case Some(1) => "one"
    case Some(x) => "x" + x
    case None => "none"
  }

  // A stable id as the tail of a cons cell, and inside an extractor.
  def onlyOne(cs: List[C]): String = cs match {
    case P(v) :: Nil => "only" + v
    case _ => "-"
  }
  def someNil(o: Option[List[C]]): String = o match {
    case Some(Nil) => "nil"
    case Some(h :: _) => "h" + h
    case None => "none"
  }

  // `case h :: t` on its own has always worked; keep it covered.
  def headTail(xs: List[Int]): Int = xs match {
    case h :: t => h + t.length
    case Nil => 0
  }

  def main(args: Array[String]): Unit = {
    val mixed: List[C] = P(1) :: Q :: P(2) :: Nil
    val qFirst: List[C] = Q :: P(9) :: Nil
    val rFirst: List[C] = R("abc") :: Q :: Nil
    val twoP: List[C] = P(3) :: P(4) :: Nil
    println(sum(mixed))
    println(sum(qFirst))
    println(sum(rFirst))
    println(sum(Nil))
    println(pairs(mixed))
    println(pairs(twoP))
    println(pairs(qFirst))
    println(second(mixed))
    println(second(qFirst))
    println(second(twoP))
    println(guarded(mixed))
    println(guarded(qFirst))
    println(guarded(twoP))
    println(bound(mixed))
    println(bound(qFirst))
    println(typed(mixed))
    println(typed(qFirst))
    println(tup((1, "a") :: Nil))
    println(tup(Nil))
    println(nested(Some(P(7))))
    println(nested(Some(Q)))
    println(nested(Some(R("z"))))
    println(nested(None))
    println(anyLit(Some(1)))
    println(anyLit(Some("s")))
    println(anyLit(None))
    println(onlyOne(P(5) :: Nil))
    println(onlyOne(mixed))
    println(onlyOne(qFirst))
    println(someNil(Some(Nil)))
    println(someNil(Some(mixed)))
    println(someNil(None))
    println(headTail(7 :: 8 :: Nil))
    println(headTail(Nil))
  }
}
