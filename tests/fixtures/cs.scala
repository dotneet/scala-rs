// SLS 5.1: a bare expression statement in a template body belongs to the
// template's initializer. For a class it runs in the primary constructor, for
// a trait in `$init$` (at mixin time, in linearization order), for an `object`
// in the module constructor — and in every case it is interleaved with the
// `val` / `var` initializers in declaration order.
object Main {
  var log: String = ""
  def note(s: String): Unit = { log = log + s + ";" }

  class A { note("A") }
  trait T1 { note("T1") }
  trait T2 extends T1 { note("T2") }
  class B extends A with T2 { note("B") }
  object O { note("O"); val v = 1 }

  // Statements and `val`s alternating: the order of the two kinds has to be
  // the source order, not "all statements then all vals" or the reverse.
  trait U {
    note("U.s1")
    val a = { note("U.a"); 1 }
    note("U.s2")
    val b = { note("U.b"); 2 }
    note("U.s3")
  }
  class C extends U {
    note("C.s1")
    val x = { note("C.x"); 10 }
    note("C.s2")
    val y = { note("C.y"); 20 }
    note("C.s3")
  }

  // A trait whose body is *only* statements still gets a `$init$`, and a
  // trait declaring an abstract member before a statement keeps both.
  trait S1 { note("S1") }
  trait S2 {
    val label: String
    note("S2:" + label)
  }
  class D extends S1 with S2 {
    val label = "d"
    note("D")
  }

  // A `var` in the body, assigned by a later statement of the same body.
  class E {
    var seen = 0
    seen = seen + 1
    note("E" + seen)
  }

  def main(args: Array[String]): Unit = {
    new B
    println(log)

    log = ""
    val c = new C
    println(log)
    println(c.a + c.b + c.x + c.y)

    log = ""
    new D
    println(log)

    log = ""
    new E
    println(log)

    // A module initializer runs exactly once, however often it is touched.
    log = ""
    println(O.v + O.v)
    println(log)
  }
}
