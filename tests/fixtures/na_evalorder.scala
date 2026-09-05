// Named arguments: the order the arguments are *evaluated* in, and the
// callees whose parameter names have to be found before they can be placed.
//
// SLS 6.6.1: arguments are evaluated left to right as written, and only then
// matched to parameters by name. Placing them in parameter order -- which is
// what resolving the names amounts to -- must not change what runs first, so
// a call that reorders anything binds its arguments to locals in front of the
// call (nsc's `NamesDefaults.transformNamedApplication`; here
// `typer::named_eval_order`). Every line below prints the order its arguments
// ran in, and is checked against real scalac 2.13.16 byte for byte.
// An `object` whose `apply` is called as `html.dropdown(...)`: the shape every
// Twirl template in gitbucket has. The members of an `object` live on its
// module class, so a reference to the object -- whose symbol is the module
// *value* -- has to be followed to the class before `apply`'s parameters can
// be found.
package p {
  package html {
    object dropdown {
      def apply(value: String = "", right: Boolean = false, filter: (String, String) = ("", ""))(
        body: String
      ): String = s"dropdown($value,$right,$filter){$body}"
    }
  }
}

object Main {
  import p.html

  var log = List.empty[String]
  def t(s: String, v: Int): Int = { log = log :+ s; v }
  def show(tag: String, v: Any): Unit = {
    println(s"$tag = $v [${log.mkString(",")}]")
    log = Nil
  }

  def f(a: Int, b: Int, c: Int): String = s"f($a,$b,$c)"
  def dflt(a: Int, b: Int = 100, c: Int = 200): String = s"dflt($a,$b,$c)"
  def mid(a: Int, b: Int = 100, c: Int): String = s"mid($a,$b,$c)"

  case class K(a: Int, b: Int, c: Int)
  class C(val a: Int, val b: Int) { override def toString = s"C($a,$b)" }

  class Box(val n: Int) {
    def two(a: Int, b: Int): String = s"two($a,$b)"
    def cur(a: Int, b: Int)(c: Int): String = s"cur($a,$b)($c)"
    def byname(a: Int, b: => Int): String = s"byname($a,$b)"
    def rep(a: Int, rest: Int*): String = s"rep($a,${rest.mkString("+")})"
  }
  // A receiver that computes: it runs before any argument, so it has to be
  // bound first when arguments move in front of it.
  def mk(): Box = { log = log :+ "recv"; new Box(0) }

  def ovl(a: Int, b: String): String = s"ovl-is($a,$b)"
  def ovl(x: Long, y: Long): String = s"ovl-ll($x,$y)"

  def main(args: Array[String]): Unit = {
    show("reorder", f(c = t("c", 3), a = t("a", 1), b = t("b", 2)))
    // A named argument that is in its own position still permits positional
    // ones after it, and nothing moves.
    show("in-place", f(t("p0", 0), c = t("c", 3), b = t("b", 2)))
    show("case-apply", K(c = t("c", 3), b = t("b", 2), a = t("a", 1)))
    show("copy", K(1, 2, 3).copy(b = t("b", 7), a = t("a", 8)))
    show("ctor", new C(b = t("b", 2), a = t("a", 1)))
    show("recv", mk().two(b = t("b", 2), a = t("a", 1)))
    show("curried", mk().cur(b = t("b", 2), a = t("a", 1))(t("c", 3)))
    // A by-name argument is not evaluated at the call site at all, so it is
    // left where it stands rather than bound to a local.
    show("byname", mk().byname(b = t("b", 2), a = t("a", 1)))
    show("varargs", mk().rep(a = t("a", 1), rest = t("r", 9)))
    show("defaults", dflt(c = t("c", 3), a = t("a", 1)))
    show("default-mid", mid(c = t("c", 3), a = t("a", 1)))
    show("overload", ovl(b = "s", a = t("a", 1)))
    show("module-apply", html.dropdown("Edit", filter = ("k", "v"), right = true) { "body" })
    show("module-qualified", p.html.dropdown(right = true, value = "Q")("b"))
    // The locals the rewrite introduces belong to whatever encloses the call,
    // so lambda-lift and the lazy-val rewrite have to see them as ordinary
    // ones: a lambda body, a method-local `def`, a `try`, a local `lazy val`,
    // and a call nested inside another call's argument.
    show(
      "in-lambda",
      List(1, 2).map(i => f(c = t("c" + i, 3), a = t("a" + i, i), b = t("b", 2))).mkString(";")
    )
    show("in-local-def", inLocalDef(5))
    show("in-try", inTry())
    show("in-local-lazy", inLocalLazy())
    show(
      "nested",
      f(c = t("nc", 3), a = f(b = t("ib", 2), a = t("ia", 1), c = t("ic", 3)).length, b = t("nb", 2))
    )
  }

  def inLocalDef(n: Int): String = {
    def g(): String = f(c = t("lc", 3), b = t("lb", n), a = t("la", 1))
    g()
  }

  def inTry(): String =
    try f(b = t("tb", 2), a = t("ta", 1), c = t("tc", 3))
    catch { case _: Throwable => "x" }

  def inLocalLazy(): String = {
    lazy val z = f(c = t("zc", 3), a = t("za", 1), b = t("zb", 2))
    z + z
  }
}
