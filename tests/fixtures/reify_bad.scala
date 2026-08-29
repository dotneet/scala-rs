// Reaching a member through a trait that extends a class must not turn into
// "anything goes": a name neither the trait nor the class declares is still an
// error, and is reported as one rather than compiled into a call that cannot
// link.

abstract class Rig {
  def tag(): String = "rig"
}

trait Cap extends Rig {
  def both(): String = tag() + tag()
}

class Gear extends Rig with Cap

object Main {
  def main(args: Array[String]): Unit = {
    val c: Cap = new Gear
    println(c.notDeclaredAnywhere())
  }
}
