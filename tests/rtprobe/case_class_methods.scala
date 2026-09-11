// Synthesized case class members: equals, hashCode, toString, copy,
// productArity/Element/ElementName/Prefix, canEqual, companion apply/unapply.
object Main {
  case class P(x: Int, y: String)
  case class Q(a: Double, b: Long, c: Char, d: Boolean, e: Float, f: Short, g: Byte)
  case class Curried(a: Int)(val b: Int)
  case class WithArr(xs: Array[Int])
  case class Gen[T](t: T, rest: List[T])
  case class Nested(p: P, q: Option[P])
  case class Empty()
  case class Vary(xs: Int*)
  class Sub(x: Int, y: String, val extra: Int) extends P(x, y)
  case class Priv private (v: Int)
  object Priv { def make(v: Int): Priv = new Priv(v * 2) }
  case class Ovr(v: Int) { override def toString = "Ovr!" + v; override def hashCode = 7 }

  def main(args: Array[String]): Unit = {
    val p = P(1, "a")
    println(p); println(p == P(1, "a")); println(p != P(2, "a")); println(p.hashCode == P(1, "a").hashCode)
    println(p.copy(y = "b")); println(p.productArity + " " + p.productElement(1) + " " + p.productPrefix)
    println(p.productElementName(0) + " " + p.productElementNames.toList + " " + p.productIterator.toList)
    println(P.unapply(p)); println(P.tupled((3, "t"))); println((P.apply _).curried(4)("c"))
    println(Q(1.5, 2L, 'c', true, 0.5f, 3, 4)); println(Q(1.5, 2L, 'c', true, 0.5f, 3, 4) == Q(1.5, 2L, 'c', true, 0.5f, 3, 4))
    println(Q(0.0, 0, 'a', false, 0, 0, 0) == Q(-0.0, 0, 'a', false, 0, 0, 0))
    println(Q(Double.NaN, 0, 'a', false, 0, 0, 0) == Q(Double.NaN, 0, 'a', false, 0, 0, 0))
    val c1 = Curried(1)(2); val c2 = Curried(1)(3)
    println(c1 + " " + (c1 == c2) + " " + c2.b)
    val arr = Array(1, 2)
    println(WithArr(arr) == WithArr(arr)); println(WithArr(arr) == WithArr(Array(1, 2)))
    println(Gen(1, List(2, 3))); println(Gen("s", Nil) == Gen("s", Nil)); println(Gen(1, Nil) == Gen(1L, Nil))
    println(Nested(p, Some(p))); println(Nested(p, None).copy(q = Some(P(9, "z"))))
    println(Empty()); println(Empty() == Empty()); println(Empty().hashCode == Empty().hashCode)
    println(Vary(1, 2, 3)); println(Vary(1, 2) == Vary(1, 2)); println(Vary().xs.length)
    val s = new Sub(1, "a", 99)
    println(s); println(s == p); println(p == s); println(s.extra)
    println(Priv.make(4)); println(Ovr(3)); println(Ovr(3).hashCode); println(Ovr(3) == Ovr(3))
    println(P(1, null)); println(P(1, null) == P(1, null)); println(P(1, null).hashCode == P(1, null).hashCode)
    println(p.canEqual(p) + " " + p.canEqual("x"))
    println(List(P(2, "b"), P(1, "z"), P(2, "a")).sortBy(pp => (pp.x, pp.y)))
    println(Set(P(1, "a"), P(1, "a"), P(2, "a")).size)
  }
}
