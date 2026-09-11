// `var x: T = _` (SLS 4.2, nsc DEFAULTINIT): the field starts at the JVM
// default of its erased type and the constructor never stores to it.
//
// Two things were wrong. The `_` was rewritten to a literal typed against
// `T`, so a type parameter (`private[this] var hd: A = _`, 17 times in
// scala/scala's own `src/library`) was `type mismatch; found: null
// required: A`. And for the types it did accept, the literal was *stored* by
// the constructor, after the superclass constructor had run -- so a value an
// overridden method wrote from there was reset to zero. scalac prints
// `5 set 0` for `B` below; this compiler printed `0 null 0`.
class Meter(val v: Int) extends AnyVal { override def toString = "Meter(" + v + ")" }

class Box[A] {
  var hd: A = _
  def isNull: Boolean = hd == null
}

class Many {
  var m: Meter = _
  var s: String = _
  var u: Unit = _
  var c: Char = _
  var d: Double = _
  var l: Long = _
  var z: Boolean = _
  var o: Option[Int] = _
  var any: Any = _
  @volatile var vol: Int = _
  private[this] var p: Int = _
  def pp: Int = p
}

abstract class Base { init(); def init(): Unit }
class Sub extends Base {
  var x: Int = _
  var s: String = _
  var y: Int = 0
  def init(): Unit = { x = 5; s = "set"; y = 7 }
}

trait TInit { init2(); def init2(): Unit }
trait TV extends TInit {
  var tx: Int = _
  var ts: String = _
  def init2(): Unit = { tx = 9; ts = "tset" }
}
class WithTV extends TV

trait T[A] { var t: A = _ }
class TS extends T[String]

object O {
  var z: Int = _
  var zs: String = _
}

abstract class It[+A] { def next(): A }

object Main {
  def mk[A](xs: List[A]): It[A] = new It[A] {
    private[this] var hd: A = _
    def next(): A = { hd = xs.head; hd }
  }
  def main(args: Array[String]): Unit = {
    val b = new Box[Int]
    println(b.isNull)
    val m = new Many
    println(m.m)
    println(m.s)
    println(m.u)
    println(m.c.toInt)
    println(m.d + " " + m.l + " " + m.z)
    println(m.o)
    println(m.any)
    println(m.vol + m.pp)
    m.m = new Meter(4)
    println(m.m)
    val sub = new Sub
    println(sub.x + " " + sub.s + " " + sub.y)
    val tv = new WithTV
    println(tv.tx + " " + tv.ts)
    val ts = new TS
    println(ts.t)
    ts.t = "q"
    println(ts.t)
    println(O.z + " " + O.zs)
    println(mk(List("a", "b")).next())
  }
}
