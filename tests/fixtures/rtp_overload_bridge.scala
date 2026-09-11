// A subclass overload with a narrower parameter is not an override, so it
// owes no erasure bridge. scala-rs read `f(x: String)` next to `A.f(x: Any)`
// as an override and emitted `B.f(Object)` forwarding to it: every call
// through `A` threw ClassCastException, and `def equals(o: E)` hijacked
// `Object.equals`. The other direction too: an override at a primitive type
// of a generic trait member did get a mixin forwarder to the trait's body
// instead of its bridge, so a call through the trait ran the wrong method.
object Main {
  class A { def f(x: Any): String = "A.f(Any)"; def g(x: AnyRef): String = "A.g" }
  class B extends A { def f(x: String): String = "B.f(String)"; def g(x: List[Int]): String = "B.g(List)" }
  trait T { def h(x: Any): String = "T.h" }
  class C extends T { def h(x: Int): String = "C.h(Int)" }
  class E { def equals(o: E): Boolean = true }
  class Parent { def who(x: Any): String = "parent-any"; def who(x: String): String = "parent-string" }
  class Child extends Parent { override def who(x: Any): String = "child-any" }
  trait Box[T] { def put(t: T): String = "box:" + t }
  class IntBox extends Box[Int] { override def put(t: Int): String = "int" + super.put(t + 1) }
  class GBox[T] { def put(t: T): String = "gbox:" + t }
  class GIntBox extends GBox[Int] { override def put(t: Int): String = "gint" + super.put(t * 2) }
  def main(args: Array[String]): Unit = {
    val a: A = new B
    println(a.f(1) + " " + a.f("s") + " " + a.g("s"))
    println((new B).f("s") + " " + (new B).f(1: Any))
    val t: T = new C
    println(t.h(1) + " " + t.h("x") + " " + new C().h(2))
    val e1 = new E; val e2 = new E
    println((e1 == e2) + " " + e1.equals(e2) + " " + (e1: Any).equals(e2) + " " + Set(e1, e2).size)
    val p: Parent = new Child
    println(p.who("s") + " " + p.who(1))
    val b: Box[Int] = new IntBox
    println(b.put(5) + " " + new IntBox().put(5))
    val g: GBox[Int] = new GIntBox
    println(g.put(5) + " " + new GIntBox().put(5))
  }
}
