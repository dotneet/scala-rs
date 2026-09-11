// Companion objects, synthetic case members and value-class equality, each
// checked against scalac 2.13.16 (crates/cli/tests/rto.rs).
import java.io._

// SD-290: a companion is Serializable exactly when its class is.
object Lone
class Plain
object Plain
class Ser extends java.io.Serializable
object Ser
trait SerT extends Serializable
object SerT
case class Cc(x: Int)

// A written `apply` with the synthetic one's signature replaces it, defaults
// included (run/t10389); an inherited concrete one does too (run/t10261).
case class Dflt(x: Int = 1)
object Dflt { def apply(x: Int = 2) = new Dflt(x) }
case class Over(x: Int = 5)
object Over { def apply(s: String): Over = new Over(s.length) }
case class Gen[A](a: A, n: Int = 7)
object Gen { def apply[A](a: A, n: Int = 8): Gen[A] = new Gen(a, n + 100) }
trait Companion[T] {
  def parse(value: String): Option[T]
  def apply(value: String): T = parse(value).getOrElse(sys.error("bad"))
}
object Parsed extends Companion[Parsed] {
  def parse(v: String) = if (v.nonEmpty) Some(new Parsed(v + "!")) else None
}
case class Parsed(value: String)
object Fn extends (Int => Fn)
case class Fn(i: Int)

// A value class keeps its own equals/hashCode over a universal trait's.
trait NoEq extends Any { override def equals(x: Any) = false }
trait NoHash extends Any { override def hashCode = -1 }
class V1(val x: Int) extends AnyVal with NoEq
class V2(val x: Int) extends AnyVal with NoHash

// A case class inherits a concrete equals/hashCode/toString from a parent
// other than Object instead of synthesizing its own.
abstract class Named { override def toString = "Named!" }
case class N(x: Int) extends Named
case class Ex(msg: String) extends Exception(msg)
case class Px(a: String) extends Proxy { def self = a }

// `@transient object` is not serialized with its enclosing instance.
class Holder extends Serializable {
  var x: Int = 0
  object A extends Serializable { var y: Int = 0 }
  @transient object B extends Serializable { var z: Int = 0 }
}

object Main {
  def ser(o: AnyRef): Array[Byte] = {
    val bytes = new ByteArrayOutputStream()
    new ObjectOutputStream(bytes).writeObject(o)
    bytes.toByteArray
  }
  def roundTrip[T <: AnyRef](o: T): T =
    new ObjectInputStream(new ByteArrayInputStream(ser(o))).readObject().asInstanceOf[T]

  def main(args: Array[String]): Unit = {
    def isSer(o: Any) = o.isInstanceOf[java.io.Serializable]
    println((isSer(Lone), isSer(Plain), isSer(Ser), isSer(SerT), isSer(Cc)))
    val s: java.io.Serializable = Ser
    println(isSer(s))

    println((new Dflt().x, Dflt().x, Dflt(3).x))
    println((Over().x, Over(6).x, Over("abc").x))
    println((Gen("a").n, Gen("a", 1).n, new Gen("b").n))
    println((Parsed("a").value, scala.util.Try(Parsed("")).isFailure))
    val f: Int => Fn = Fn
    println((Fn(3), f(4)))

    val v1 = new V1(71)
    val v2 = new V2(71)
    println((v1 == v1, v1.## == 71.##, v2 == v2, v2.## == 71.##))

    println((N(1), N(1) == N(1)))
    println((Ex("boom"), Ex("a") == Ex("a")))
    println((Px("p") == Px("p"), Px("p") == "p"))

    val h = new Holder
    h.x = 1
    h.A.y = 2
    h.B.z = 3
    val h2 = roundTrip(h)
    println((h2.x, h2.A.y, h2.B.z))
  }
}
