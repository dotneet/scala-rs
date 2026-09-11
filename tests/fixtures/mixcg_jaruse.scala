// Uses `mixcg_jarlib/MixcgLib.scala` from a jar.
import mixcglib._

class Hush extends RuntimeException("hush") with Silent
class Stacked extends Base with Loud
// The superclass declares `sides` abstractly: the JVM resolves the class's
// abstract method ahead of `Square`'s default, so the class needs the
// forwarder all the same.
class Tile extends Shape with Square

object Main {
  def main(args: Array[String]): Unit = {
    println(new Hush().getStackTrace.length)
    println(new Stacked().label)
    println(new Tile().sides)
    val s: Shape = new Tile
    println(s.sides)
  }
}
