// `==` on references: nsc calls `equals` (after a null test) unless both
// operands may hold a boxed number, and only then goes through
// `BoxesRunTime.equals` -- whose reference shortcut skips a user `equals`.
// A case class compares fields with `==`, so boxed numbers in generic or
// `Any` fields compare cooperatively.
object Main {
  class Counted(val v: Int) {
    var eqCalls = 0
    override def equals(o: Any) = { eqCalls += 1; o.isInstanceOf[Counted] && o.asInstanceOf[Counted].v == v }
    override def hashCode = v
  }
  class Never { override def equals(o: Any) = false }
  case class Gen[T](t: T, rest: List[T])
  case class AnyF(x: Any)
  def main(args: Array[String]): Unit = {
    val k1 = new Counted(1); val k2 = new Counted(1)
    println((k1 == k2) + " " + k1.eqCalls + " " + (k1 == k1) + " " + k1.eqCalls + " " + (k1 == null) + " " + k1.eqCalls)
    val n = new Never
    println((n == n) + " " + (n != n) + " " + (n eq n))
    println(Gen(1, Nil) == Gen(1L, Nil))
    println(Gen('a', Nil) == Gen(97, Nil))
    println(AnyF(1) == AnyF(1.0))
    println(AnyF(1).hashCode == AnyF(1.0).hashCode)
    println(Gen("x", List("y")) == Gen("x", List("y")))
    val b = BigInt(7)
    println((10 == BigInt(10)) + " " + (7.0 == b) + " " + (b == 7L) + " " + ((b: Any) == 7))
    val xs: List[Any] = List(1, 1L, 1.0, 'x', "1")
    println(xs.map(x => xs.count(_ == x)).mkString(","))
    println((() == ()) + " " + (null == n) + " " + (n == null))
  }
}
