// Objects declared inside traits and classes are per instance and lazily
// created; companion-like nested objects; an object extending its
// enclosing trait's inner class.
object Main {
  var created = 0
  trait Module { val prefix: String; object Helper { created += 1; def tag(s: String) = prefix + s }; case class Item(n: Int) }
  class M1 extends Module { val prefix = "m1:" }
  class M2 extends Module { val prefix = "m2:" }
  class Outer { class In(val v: Int); object In { def zero = new In(0) }; object Special extends In(42) }
  def main(args: Array[String]): Unit = {
    val a = new M1; val b = new M2
    println(created)
    println(a.Helper.tag("x") + " " + b.Helper.tag("y") + " " + a.Helper.tag("z"))
    println(created + " " + (a.Helper eq a.Helper) + " " + ((a.Helper: AnyRef) eq (b.Helper: AnyRef)))
    val i1 = a.Item(1); val i2 = a.Item(1)
    println(i1 + " " + (i1 == i2) + " " + b.Item(2))
    val o = new Outer
    println(o.In.zero.v + " " + o.Special.v + " " + (o.Special: o.In).v)
  }
}
