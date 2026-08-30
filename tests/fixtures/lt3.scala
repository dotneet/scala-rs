// A local trait reading enclosing-method locals. A trait has no constructor,
// so the captured value cannot be a constructor parameter the way a local
// class's is: the trait declares an accessor, every class mixing it in
// captures the same local and implements the accessor from its own field.
object Main {
  def capturesVal(): Unit = {
    val n = 42
    val s = "s"
    trait Cap { def show = "cap" + n + s }
    class CapC extends Cap
    println(new CapC().show)
    println((new Cap {}).show)
  }

  def capturesParam(p: Int, q: String): Unit = {
    trait Cap { def show = q + p }
    class CapC extends Cap
    println(new CapC().show)
  }

  def capturesVar(): Unit = {
    var counter = 0
    trait Bump { def bump(): Int = { counter += 1; counter } }
    class BumpC extends Bump
    val b = new BumpC
    println(b.bump())
    println(b.bump())
    counter += 10
    println(b.bump())
  }

  def captureThroughTraitVal(): Unit = {
    val base = "base"
    trait WithVal { val tag = base + "!"; def show = tag + "/" + base }
    class WV extends WithVal
    println(new WV().show)
    println(new WV().tag)
  }

  def captureInherited(): Unit = {
    val hidden = "h"
    trait Inner { def i = hidden + "i" }
    trait Outer extends Inner { def o = i + "o" }
    class Both extends Outer
    println(new Both().o)
  }

  def main(args: Array[String]): Unit = {
    capturesVal()
    capturesParam(7, "p")
    capturesVar()
    captureThroughTraitVal()
    captureInherited()
  }
}
