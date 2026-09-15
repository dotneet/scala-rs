import erascglib._

class Y extends X { type T = Int; def f(t: Int): Int = t + 1 }
class Z extends TI[Long] { def h(a: Long): Long = a * 3 }
class Q extends P[Int](7) { override def get: Int = 8 }

object Main {
  def main(args: Array[String]): Unit = {
    assert(First.Regex.unapply(First.Regex()))
    assert(First.Regex.label == "first")
    assert(Second.Regex.unapply(Second.Regex(42)).contains(42))
    assert(Second.Regex.label == "second")
    assert(Views.size("abc") == 3)
    assert(Views.integers(_ => List(1, 2)) == 2)
    val symbolic = SymbolicUse.identity(new Symbolic.::(42, "tail"))
    assert(Views.nested(new Box[List[_]](List(1, "x"))) == 2)
    assert(!new ValueBox(1).equals(1))
    assert(new ValueBox(1).equals(new ValueBox(1)))
    assert(new ValueBox(1).hashCode() == 1.hashCode())
    assert(Views.mixed(new Box[(_, List[_])]((1, List(2, "x")))) == 2)
    assert(symbolic.head == 42 && symbolic.tail == "tail")
    val p = new P(3)
    println(p.a + p.get + p.twice(4))
    p.v = 5
    println(p.v)
    println(new Y().f(2))
    val z: TI[Long] = new Z
    println(z.h(2L))
    println((new Q: P[Int]).get)
    println(Fns.id(9) + 1)
    println(Fns.mk('c'))
    println(Fns.arr(Array(4, 5)))
    val c = new C
    val d = c.mkD
    println(d.foo("abcd"))
    val s = new SB("abc")
    println(s"${s.len} ${s.get} ${s.a}")
  }
}
