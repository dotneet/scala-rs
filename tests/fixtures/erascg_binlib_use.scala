import erascglib._

class Y extends X { type T = Int; def f(t: Int): Int = t + 1 }
class Z extends TI[Long] { def h(a: Long): Long = a * 3 }
class Q extends P[Int](7) { override def get: Int = 8 }

object Main {
  def main(args: Array[String]): Unit = {
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
    println(s.len + " " + s.get + " " + s.a)
  }
}
