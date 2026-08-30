// Traits declared inside a method body: the mixin forwarders, the trait
// `val`/`lazy val` fields and the `$init$` call have to be generated exactly
// as for a top-level trait. They were not generated at all, so every class
// mixing a local trait in failed at run time with `AbstractMethodError`.
trait TopT { def greet: String = "top:" + tag; def tag: String }

object Main {
  def basic(): Unit = {
    trait L {
      val v: String
      val fixed = "F"
      lazy val w = v + "!"
      def plain = v + "?"
    }
    class LC extends L { val v = "x" }
    val c = new LC
    println(c.plain)
    println(c.w)
    println(c.fixed)
    // Through the interface, so the forwarder really is the one being called.
    val l: L = c
    println(l.plain)
    println(l.w)
  }

  // A local class implementing a *top-level* trait already worked; keep it.
  def localClassTopTrait(): Unit = {
    class LocalC extends TopT { def tag = "lc" }
    println(new LocalC().greet)
  }

  def anonymous(): Unit = {
    trait Q { def q = "q" }
    println((new Q {}).q)
    class P
    println((new P with Q).q)
  }

  def inBlockAndBranch(): Unit = {
    val r = {
      trait T { def t = "blockT" }
      class C extends T
      new C
    }
    println(r.t)
    if (r.t.length > 0) {
      trait U { def u = "ifU" }
      class D extends U
      println(new D().u)
    }
  }

  // Any block, not just a method body: a lambda, a `match` case, a `while`
  // body, a `try` block.
  def everyBlockKind(): Unit = {
    val f = (x: Int) => {
      trait Lam { def l = "lam" + x }
      class LamC extends Lam
      new LamC().l
    }
    println(f(3))
    println(1 match {
      case _ =>
        trait M { def mm = "mm" }
        class MC extends M
        new MC().mm
    })
    var i = 0
    while (i < 2) {
      trait W { def w = "w" + i }
      class WC extends W
      println(new WC().w)
      i += 1
    }
    val t = try {
      trait Y { def y = "y" }
      class YC extends Y
      new YC().y
    } catch { case _: Throwable => "no" }
    println(t)
  }

  def main(args: Array[String]): Unit = {
    basic()
    localClassTopTrait()
    anonymous()
    inBlockAndBranch()
    everyBlockKind()
  }
}
