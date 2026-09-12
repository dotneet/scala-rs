// Value-class and primitive receivers that need a real instance, ascribed
// function values in call position, and pattern binders whose erased type is
// narrower than the scrutinee's.

// A universal trait's member reached through a source value class.
trait Show extends Any {
  def show: String = "<" + toString + ">"
  def twice(n: Int): Int = n * 2
}
class V(val x: Int) extends AnyVal with Show {
  override def toString = "V" + x
}

// `(F: Int => F)(4)`: the companion itself is the function.
class F(val i: Int) { override def toString = "F" + i }
object F extends (Int => F) {
  def apply(i: Int): F = new F(i)
}
object G extends (Int => Int) {
  def apply(x: Int): Int = x + 1
}

object Dingus { def IamDingus = 5 }
class A { def a = 7 }

// `def unapply(u: Unit)` takes the `Unit` scrutinee as a `BoxedUnit`.
object UnitX { def unapply(u: Unit): Option[Int] = Some(42) }
object AnyX { def unapply(a: Any): Option[String] = Some("any:" + a) }

object Main {
  val a1 = new A
  def single(x: Any): Int = x match {
    case d: Dingus.type => d.IamDingus
    case y: a1.type     => y.a
    case _              => 0
  }

  def main(args: Array[String]): Unit = {
    // RichInt/RichLong/RichDouble/RichChar members that only the universal
    // trait `ScalaNumericAnyConversions` declares: no `$extension` exists.
    println(5.isValidByte)
    println(500.isValidByte)
    val x: Int = 7
    println(x.isValidShort)
    println(70000.isValidShort)
    println(3L.isValidInt)
    println(7L.isValidByte)
    println(3.0.isWhole)
    println(3.5.isValidInt)
    println('a'.isValidByte)
    println(5.toByte.isValidChar)
    println(12.max(3).isValidByte)
    val xs = List(1, 200, 70000)
    println(xs.map(_.isValidShort))
    println(xs.filter(_.isValidByte))
    // The directly lowered members keep working.
    println(3.sign + " " + 3.compare(4) + " " + 'a'.toInt + " " + 3.abs)

    val v = new V(3)
    println(v.show + " " + v.twice(4))

    println((F: Int => F)(4))
    val f: Int => F = F
    println(f(5))
    println(List(1, 2).map(F))
    println((G: Int => Int)(4))
    val g: Int => Int = _ * 2
    println((g: Int => Int)(4))

    println(single(Dingus) + " " + single(a1) + " " + single(new A))

    val UnitX(y) = ()
    println(y)
    () match { case AnyX(s) => println(s) }
    val u: Unit = ()
    u match { case UnitX(z) => println(z + 1) }
  }
}
