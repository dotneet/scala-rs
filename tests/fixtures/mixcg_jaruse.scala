// Uses `mixcg_jarlib/MixcgLib.scala` from a jar.
import mixcglib._

class Hush extends RuntimeException("hush") with Silent
class Stacked extends Base with Loud
// The superclass declares `sides` abstractly: the JVM resolves the class's
// abstract method ahead of `Square`'s default, so the class needs the
// forwarder all the same.
class Tile extends Shape with Square

// A trait of this run and a binary one layered over the same member: the
// linearization decides, in both orders.
trait Src extends Base { override def label: String = "src(" + super.label + ")" }
class SrcThenLoud extends Base with Src with Loud
class LoudThenSrc extends Base with Loud with Src

// Two binary traits, neither a subtrait of the other: `B1` wins.
class AB extends A0 with B1
class BA extends B1 with A0

object Main {
  def main(args: Array[String]): Unit = {
    println(new Hush().getStackTrace.length)
    println(new Stacked().label)
    println(new Tile().sides)
    val s: Shape = new Tile
    println(s.sides)
    println(new SrcThenLoud().label)
    println(new LoudThenSrc().label)
    println(new AB().m + " " + new BA().m)
    val b0: Base0 = new AB
    println(b0.m)
  }
}
