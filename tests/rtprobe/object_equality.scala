// User-defined equals/hashCode: `==` dispatches to equals (with a null
// check), `eq` is reference identity, hash-based collections consult
// hashCode, and `equals` overloads that do not override.
object Main {
  class Pt(val x: Int, val y: Int) {
    override def equals(o: Any): Boolean = o match { case p: Pt => p.x == x && p.y == y; case _ => false }
    override def hashCode: Int = x * 31 + y
    override def toString = s"Pt($x,$y)"
  }
  class BadEq(val v: Int) { def equals(o: BadEq): Boolean = o.v == v }  // overload, not override
  class AlwaysEq { override def equals(o: Any) = true; override def hashCode = 1 }
  class Counted(val v: Int) { var eqCalls = 0; override def equals(o: Any) = { eqCalls += 1; o.isInstanceOf[Counted] && o.asInstanceOf[Counted].v == v }; override def hashCode = v }
  def main(args: Array[String]): Unit = {
    val a = new Pt(1, 2); val b = new Pt(1, 2); val c = new Pt(2, 1)
    println((a == b) + " " + (a eq b) + " " + (a != c) + " " + (a ne b) + " " + a.equals(b) + " " + (a.## == b.##))
    println(Set(a, b, c).size + " " + Map(a -> "first", b -> "second").size + " " + List(a, b, c).distinct.size + " " + List(a).contains(b))
    val x = new BadEq(1); val y = new BadEq(1)
    println((x == y) + " " + x.equals(y) + " " + (x: Any).equals(y) + " " + List(x).contains(y))
    val ae = new AlwaysEq
    println((ae == "anything") + " " + (ae == null) + " " + ("anything" == ae) + " " + ((ae: Any) == 5))
    val k1 = new Counted(1); val k2 = new Counted(1)
    println((k1 == k2) + " eqCalls=" + k1.eqCalls + " " + (k1 == k1) + " eqCalls=" + k1.eqCalls)
    println((k1 == null) + " eqCalls=" + k1.eqCalls)
    val hs = scala.collection.mutable.HashSet(new Pt(0, 0))
    println(hs.contains(new Pt(0, 0)) + " " + hs.contains(new Pt(0, 1)))
    val s1 = new String("abc"); val s2 = new String("abc")
    println((s1 == s2) + " " + (s1 eq s2))
    val l1 = List(1, 2); val l2 = List(1, 2)
    println((l1 == l2) + " " + (l1 eq l2) + " " + (Nil eq List()) + " " + (None eq None))
    println((a: AnyRef) == (b: AnyRef))
    val arrA = Array(1); val arrB = Array(1)
    println((arrA == arrB) + " " + (arrA.toSeq == arrB.toSeq))
  }
}
