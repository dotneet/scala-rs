// Three shapes slick's own run-time failures came from, none of which needs
// the collections.
//
// 1. `this` in a template's own constructor invocation. The arguments of
//    `new C(this.x) { … }` belong to the *enclosing* expression, so `this`
//    there is the enclosing template's, not the anonymous class's — whose
//    slot 0 is still uninitialised at that point. `class D extends
//    Base(this.toString)` is the same rule.
// 2. A `def this()` that leaves defaulted parameters out. The call reaches the
//    class file as one `invokespecial`, so the defaults have to be filled in
//    at the call site exactly as a `new` fills them.
// 3. Overloaded concrete trait methods. A class mixing the trait in needs a
//    mixin forwarder per overload, not per name.

class Base(val tag: String) {
  def describe: String = "Base(" + tag + ")"
}

class Outer(val nm: String) {
  override def toString: String = "Outer(" + nm + ")"
  val anon: Base = new Base(this.toString) {
    override def describe: String = "anon:" + tag
  }
  class Inner extends Base(this.toString)
  val inner: Base = new Inner
}

object Single {
  override def toString: String = "Single!"
  val anon: Base = new Base(this.toString) {
    override def describe: String = "anonS:" + tag
  }
  class Inner extends Base(this.toString)
  val inner: Base = new Inner
}

class Defaults(
  val url: String,
  val user: String = "anon",
  val flag: Boolean = false,
  val n: Int = 7,
  val label: String = "L" + "D"
) {
  def this() = this("u0")
  override def toString: String =
    "Defaults(" + url + "," + user + "," + flag + "," + n + "," + label + ")"
}

trait Overloads {
  def pick(a: Int): String = "int:" + a
  def pick(a: String): String = "str:" + a
  def pick(a: Int, b: Int): String = "two:" + (a + b)
}

object Uses extends Overloads

class UsesToo extends Overloads

object Main {
  def main(args: Array[String]): Unit = {
    val o = new Outer("x")
    println(o.anon.describe)
    println(o.inner.describe)
    println(Single.anon.describe)
    println(Single.inner.describe)

    println(new Defaults("a"))
    println(new Defaults("a", "b"))
    println(new Defaults())

    val ov: Overloads = Uses
    println(ov.pick(1))
    println(ov.pick("s"))
    println(ov.pick(2, 3))
    val ov2: Overloads = new UsesToo
    println(ov2.pick(4))
    println(ov2.pick("t"))
    println(ov2.pick(5, 6))
  }
}
