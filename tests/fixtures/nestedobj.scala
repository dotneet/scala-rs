// A member `object` of a class is one instance per enclosing instance: it
// carries an `$outer` and the enclosing class hands it out through an
// accessor. scalac 2.13.16's own output is in expected/nestedobj.txt.
object Main {
  class Outer(val v: Int) {
    trait T { def d = v * 2 }
    object O extends T
    object P { def q = v + 1; def viaQ = Q.z }
    object Q { def z = v * 3; def viaP = P.q }
    class C { def r = v + 2 }
    object R { def w = Outer.this.v + 100 }
    object N { val nv = v + 7; object Deep { def f = nv * 2 } }
    // A local `object` that reads nothing outside itself still compiles.
    def localOk(k: Int): Int = { object L { def g = 7 }; L.g + k }
  }

  class A(val a: Int) {
    class B(val b: Int) {
      object C2 { def f = a + b }
    }
  }

  def main(args: Array[String]): Unit = {
    val o = new Outer(10)
    println(o.P.q)
    println((new o.C).r)
    println(o.O.d)
    println(o.P.viaQ)
    println(o.Q.viaP)
    println(o.R.w)
    println(o.N.Deep.f)
    // One instance per enclosing instance, created once.
    println(o.P eq o.P)
    val o2 = new Outer(1)
    println(o.P eq o2.P)
    println(o2.P.q)
    val aa = new A(1)
    val bb = new aa.B(2)
    println(bb.C2.f)
    println(o.localOk(3))
  }
}
