// The shapes that decide *which* of nsc's two synthesized `hashCode` bodies a
// case class gets. `SyntheticMethods.chooseHashcode` writes the MurmurHash3
// mix chain out inline when at least one case accessor has a primitive value
// type (`Unit` counts), and otherwise forwards the whole thing to
// `ScalaRunTime$.MODULE$._hashCode(this)`.
//
// A value-class accessor is *not* primitive for that test even though the
// field is stored as its erased underlying `Int` -- `Box` below goes to the
// runtime, and in `Mixed` the value class is boxed back up with `new Meters(..)`
// before `Statics.anyHash`. Reading the raw underlying value instead happens to
// give the same number for a value class over `Int`, which is why this fixture
// also carries `MetersL` over `Long` at `-1L`: `Statics.longHash(-1L)` is `-1`
// (the value fits in an `Int`, so it is returned as is) while
// `java.lang.Long.hashCode(-1L)`, which is what the boxed `MetersL` answers,
// is `0`. That one field tells the two paths apart.
//
// Every value here is diffed against real scalac 2.13.16 compiling the same
// source (`crates/cli/tests/caseabi.rs`).

class Meters(val u: Int) extends AnyVal
class MetersL(val u: Long) extends AnyVal

case class Inner(k: Int)
case class Box(m: Meters, b: String)
case class Mixed(m: Meters, n: Int)
case class BoxL(m: MetersL, n: Int)
case class Cell[T](t: T)
case class GenMix[T](t: T, n: Int)
case class AnyF(a: Any)
case class Nested2(p: Inner, n: Int)
case class OnlyUnit(u: Unit)
case class Pair(a: String, b: String)

object Main {
  def main(args: Array[String]): Unit = {
    println(Box(new Meters(3), "b").hashCode)
    println(Mixed(new Meters(3), 4).hashCode)
    println(BoxL(new MetersL(-1L), 4).hashCode)
    println(new MetersL(-1L).hashCode)
    println(BoxL(new MetersL(9999999999L), 4).hashCode)
    println(Cell(42).hashCode)
    println(Cell("s").hashCode)
    println(GenMix("t", 1).hashCode)
    println(AnyF(1.0).hashCode)
    println(AnyF(1).hashCode)
    println(Nested2(Inner(2), 3).hashCode)
    println(OnlyUnit(()).hashCode)
    println(Pair("a", "b").hashCode)

    // `1.0.##` and `1.##` agree in Scala, and a case class's hash has to
    // inherit that: `Statics.anyHash` on the boxed `Double` is why.
    println(AnyF(1.0).hashCode == AnyF(1).hashCode)

    // The point of the whole exercise: a `java.util.HashMap` keyed by a case
    // class finds the entry back.
    val m = new java.util.HashMap[Pair, String]()
    m.put(Pair("a", "b"), "found")
    println(m.get(Pair("a", "b")))
    println(m.get(Pair("a", "c")))
  }
}
