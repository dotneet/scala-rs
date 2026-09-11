// Singleton and literal type tests, and which identifier patterns bind,
// checked against scalac 2.13.16 (crates/cli/tests/rto.rs).
class A { object X }
class Sym(val n: String) {
  override def equals(o: Any) = o match { case s: Sym => s.n == n; case _ => false }
  override def hashCode = n.hashCode
}
class Outer {
  def self(o: Any) = o match { case _: this.type => "me"; case _ => "other" }
  def selfI(o: Any) = o.isInstanceOf[this.type]
  class Inner { def outerTest(o: Any) = o match { case _: Outer.this.type => "outer"; case _ => "no" } }
}

object Main {
  val a, b = new A
  val bippy = new Sym("b")
  val imp = new Sym("b")
  // `_: p.type` is `p eq x`; a stable identifier pattern is `p == x`.
  def f(s: Sym) = s match { case _: bippy.type => 1; case `bippy` => 2; case _ => 3 }
  def g(s: Any) = s match { case y: bippy.type => y.n + "!"; case _ => "none" }
  object Foo
  object Bar

  val ⅰ_ⅲ = "roman"
  val ǅul = "title"

  def main(args: Array[String]): Unit = {
    import a.X
    def t(z: AnyRef) = z.isInstanceOf[X.type]
    println((t(a.X), t(b.X), t(new Object)))
    println((f(bippy), f(imp), f(new Sym("c"))))
    val o: Any = imp
    println((o.isInstanceOf[bippy.type], (bippy: Any).isInstanceOf[bippy.type]))
    println((g(bippy), g(imp), g(null)))

    println(((0: Any) match { case _: 0 => true; case _ => false }, (0: Any) match { case _: 1 => false; case _ => true }))
    println(((null: Any) match { case _: 0 => false; case _ => true }))
    println((("foo": Any) match { case _: "foo" => true; case _ => false }, ("foo": Any) match { case _: "bar" => false; case _ => true }))
    println(((Foo: Any) match { case _: Foo.type => true; case _ => false }, (Foo: Any) match { case _: Bar.type => false; case _ => true }))
    val one: Any = 1000
    println(((1000: Any) match { case _: one.type => "eqv"; case _ => "ne" }, (1000: Any).isInstanceOf[one.type]))
    val i = 3
    println((3 match { case z: i.type => z + 1; case _ => 0 }, (4: Int) match { case _: i.type => 1; case _ => 0 }))
    println(((1L: Any).isInstanceOf[1], (1: Any).isInstanceOf[1], (2: Any).isInstanceOf[1], ("s": Any).isInstanceOf["s"]))
    println(((1L: Any) match { case _: 1 => "m"; case _ => "n" }, (1: Any) match { case q: 1 => q; case _ => 0 }))
    val oo = new Outer
    println((oo.self(oo), oo.self(new Outer), oo.selfI(oo), oo.selfI(new Outer)))
    val in = new oo.Inner
    println((in.outerTest(oo), in.outerTest(new Outer)))

    // A lower-case letter *number* does not start a variable name, a title
    // case letter neither; `ª` (`Other_Lowercase`, a letter) does.
    val r = "foo" match {
      case ⅰ_ⅲ => "constant"
      case ǅul => "constant"
      case ªpple => "bound " + ªpple
    }
    println(r)
  }
}
