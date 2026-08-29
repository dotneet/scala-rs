// Member classes of a *trait* need an `$outer` just like member classes of a
// class: the trait is an interface, so `x`, `v`, `lz` and the type member `A`
// are only reachable through the enclosing instance.
trait T {
  def x: Int = 1
  val v: String = "v"
  lazy val lz: Int = x + 6
  type A = Int

  class Inner(val tag: String) {
    def y: A = x + lz
    def s: String = v + tag
    // Two levels deep: `Deep.z` reaches `Inner.y` through one `$outer` and
    // `T.x` through two.
    class Deep {
      def z: Int = y + x
      def outerTag: String = tag
    }
    def deep: Deep = new Deep
  }

  def make(tag: String): Inner = new Inner(tag)
}

class C extends T {
  override def x: Int = 10
  def viaThis: Int = new Inner("c").y
}

trait U extends T {
  def viaTrait: Int = new Inner("u").y
}

class D extends U {
  def own: Int = make("d").deep.z
}

// A class nested in an object that extends a class member of a trait: the
// super constructor takes the enclosing instance, and it is the object.
object Holder extends T {
  class Row(tag: String) extends Inner(tag) {
    def label: String = s + ":" + y
  }
}

// The plain (non-trait) nesting must keep working.
class Plain {
  def base: Int = 100
  class Sub { def v: Int = base + 1 }
}

object Main {
  def main(args: Array[String]): Unit = {
    val c = new C
    println(c.viaThis)
    println(c.make("m").y)
    println(c.make("m").s)
    // `new i.Deep` names its enclosing instance explicitly.
    val i = c.make("p")
    println(new i.Deep().z)
    println(i.deep.outerTag)

    val d = new D
    println(d.viaTrait)
    println(d.make("q").s)
    println(d.own)

    println(new Holder.Row("r").label)
    println(Holder.make("h").y)

    val p = new Plain
    println(new p.Sub().v)
  }
}
