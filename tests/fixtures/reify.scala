// A trait that extends a class is an interface in bytecode, and an interface
// cannot extend a class -- the class in between simply does not appear in the
// trait's class-file header. So a member reached through such a trait must be
// called on the class that declares it, after a `checkcast` to it, while the
// trait's own members are called with `invokeinterface`. Getting either of the
// two wrong is not a type error: it is `NoSuchMethodError` /
// `IncompatibleClassChangeError` at the first call.
//
// This is the shape `scala.reflect.api.JavaUniverse` has (`Constant()` is
// declared on `scala.reflect.api.Constants`, reachable only through the
// abstract class `Universe`), which is why it matters for macros.

abstract class Rig {
  def tag(): String = "rig"
  def size(): Int = 3
}

trait Cap extends Rig {
  def both(): String = tag() + "/" + tag()
  def wider(): Int = size() * 2
}

class Gear extends Rig with Cap {
  override def tag(): String = "gear"
}

object Main {
  def main(args: Array[String]): Unit = {
    val g = new Gear
    // Receiver typed as the trait: `tag` is declared by the class `Rig`,
    // `both` by the interface `Cap`.
    val c: Cap = g
    println(c.tag())
    println(c.both())
    println(c.wider())
    // Receiver typed as the class: both are reachable the ordinary way.
    val r: Rig = g
    println(r.tag())
    println(r.size())
  }
}
