// A class already implements a trait's `final val`; a module that extends that
// class must NOT declare the accessor again.
//
// nsc's mixin phase only visits `clazz.mixinClasses` -- the prefix of the
// linearization *before* the superclass -- so a trait the superclass already
// carries gets nothing here. We visited the whole linearization, so
// `object Leaf extends Base` re-emitted `v()` and the `T$_setter_$v_$eq` that
// `Base` declares `final`, and the JVM refused to load the class:
//
//   java.lang.IncompatibleClassChangeError: class Leaf$ overrides final method
//   Base.T$_setter_$v_$eq(Ljava/lang/String;)V
//
// cats died this way before any of its code ran: `object all extends
// AllInstancesBinCompat` over `CoreDurationInstances`' `implicit final val
// catsStdShowForDurationUnambiguous`.
trait T {
  implicit final val v: String = "v"
  final val w: Int = 7
  def both: String = v + w
}

class Base extends T

object Leaf extends Base

// The same shape with a `var`, a `lazy val` and a member object, which the field
// / accessor / `$init$` passes each handle separately.
trait U {
  var n: Int = 1
  lazy val lz: String = "lz" + n
  object Inner { def hi: String = "inner" + n }
}

class UBase extends U
object ULeaf extends UBase

// A trait mixed in by the *module* itself is still the module's own business.
trait Extra { def extra: String = "extra" }
object Mixed extends UBase with Extra

object Main {
  def main(args: Array[String]): Unit = {
    println(Leaf.v)
    println((Leaf: Base).v)
    println(Leaf.both)
    println(Leaf.w)
    ULeaf.n = 4
    println(ULeaf.n)
    println(ULeaf.lz)
    println(ULeaf.Inner.hi)
    println(Mixed.extra)
    println(Mixed.lz)
  }
}
