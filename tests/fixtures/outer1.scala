// Anonymous / local classes reaching the *enclosing* class.
//
//  * a super-constructor argument of an anonymous class reads the enclosing
//    instance while `this` is still `uninitializedThis`,
//  * `private` / `private[this]` members of the enclosing class are named
//    from an anonymous class, from a lambda body and from the companion,
//  * an enclosing `private[this] var` is assigned from an anonymous class,
//  * a lambda body builds an anonymous class that needs the enclosing
//    instance for its own `$outer`.
object Main {
  abstract class Base(val tag: String) {
    def describe: String
  }

  class Outer(val n: Int) {
    private[this] val secret: Int = n * 2
    private val hidden: Int = n + 100
    private def helper: Int = n + 1000
    private[this] var bumped: Int = 0

    // The super-constructor argument runs before `$outer` may be read off
    // `this`; scalac stores `$outer` first and reads the `<init>` parameter.
    def mk(): Base = new Base("tag" + n) {
      def describe: String = tag + "/" + n + "/" + secret
    }

    def qualified(): String = new AnyRef {
      override def toString: String = "" + Outer.this.secret
    }.toString

    def privates(): String = new AnyRef {
      override def toString: String = "" + hidden + ":" + helper
    }.toString

    def bump(): String = new AnyRef {
      override def toString: String = { bumped = bumped + 1; "" + bumped }
    }.toString

    def viaLambda(): String = {
      val f = () => new AnyRef { override def toString: String = "" + secret }
      f().toString
    }

    def lambdaPrivate(): String = {
      val f = () => hidden + helper
      "" + f()
    }

    class Inner {
      def read: String = "" + secret + "," + hidden + "," + helper
    }
    def inner(): String = new Inner().read
  }

  // `private[this] def y` is not inherited, so `Q` may declare its own `y`.
  // scalac renames `P`'s to `Main$P$$y`; publishing it under the source name
  // would let `Q.y` override it and `new Q().mk()` would print 9.
  class P {
    private[this] def y: Int = 2
    def mk(): String = new AnyRef { override def toString: String = "" + y }.toString
  }
  class Q extends P {
    def y: Int = 9
  }

  class Holder(val v: Int)
  object Holder {
    private[this] val note: String = "note"
    def mk(): String = new AnyRef { override def toString: String = note }.toString
  }

  trait T {
    private[this] val b: Int = 3
    def mk(): String = new AnyRef { override def toString: String = "" + b }.toString
  }
  class C extends T

  def main(args: Array[String]): Unit = {
    val o = new Outer(7)
    println(o.mk().describe)
    println(o.qualified())
    println(o.privates())
    println(o.bump())
    println(o.bump())
    println(o.viaLambda())
    println(o.lambdaPrivate())
    println(o.inner())
    println(new P().mk())
    println(new Q().mk())
    println(new Q().y)
    println(Holder.mk())
    println(new C().mk())
  }
}
