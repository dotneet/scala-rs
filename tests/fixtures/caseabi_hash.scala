// The `hashCode` a case class gets synthesized (SLS 5.3.2). Under
// `--scala-library` this must be nsc's MurmurHash3 value, byte for byte, or a
// case class we compile and one scalac compiles hash differently and break any
// `HashMap` that sees both. Under `--no-scala-library` there is no
// `scala.runtime.Statics`, so the 31-fold stays and the numbers differ by
// design -- which is why the two modes have separate expected files
// (`caseabi_hash.txt` / `caseabi_hash_priv.txt`).
//
// Only `java.lang` types appear here so the fixture compiles in both modes;
// the value-class / `Any` / `Array` / generic shapes that decide *which* of
// nsc's two `hashCode` shapes gets emitted live in `caseabi_hash_lib.scala`.

case class Point(x: Int, y: String)
case class Zero()
case class One(a: Int)
case class Wide(a: Int, b: Long, c: Double, d: Boolean, e: Char, f: String)
case class Floaty(f: Float, d: Double)
case class Longy(l: Long)
case class Bytes(b: Byte, s: Short, c: Char)
case class UnitF(u: Unit, i: Int)
case class OnlyStr(s: String)
case class Nested(p: One, n: Int)
case object Solo

// A hand-written `hashCode` still wins over the synthesized one, in both modes.
case class Custom(x: Int) {
  override def hashCode(): Int = 4242
}

object Main {
  def main(args: Array[String]): Unit = {
    println(Point(1, "a").hashCode)
    println(Zero().hashCode)
    println(One(7).hashCode)
    println(Wide(1, 2L, 3.5, true, 'z', "q").hashCode)
    println(Floaty(1.5f, 2.5).hashCode)
    println(Longy(9999999999L).hashCode)
    println(Bytes(1, 2, 'c').hashCode)
    println(UnitF((), 5).hashCode)
    println(OnlyStr("s").hashCode)
    println(Nested(One(2), 3).hashCode)
    println(Solo.hashCode)
    println(Custom(1).hashCode)

    // Whatever the numbers are, `hashCode` has to agree with `equals`: equal
    // values hash equal, and a different field gives a different hash.
    println(Point(1, "a").hashCode == Point(1, "a").hashCode)
    println(Point(1, "a") == Point(1, "a"))
    println(Point(1, "a").hashCode == Point(2, "a").hashCode)
    println(Wide(1, 2L, 3.5, true, 'z', "q").hashCode == Wide(1, 2L, 3.5, true, 'z', "q").hashCode)
    println(Wide(1, 2L, 3.5, false, 'z', "q").hashCode == Wide(1, 2L, 3.5, true, 'z', "q").hashCode)
    println(Longy(1L).hashCode == Longy(2L).hashCode)
    println(Custom(1).hashCode == Custom(2).hashCode)
  }
}
