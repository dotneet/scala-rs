// Every class below is rejected by scalac 2.13.16, each on its own line.
import java.{util => ju}

// `java.lang.Object`'s `getClass`, `notify`, `notifyAll` and `wait` are final.
class R1 { def notify(): Unit = () }
class R2 { override def getClass(): Class[R2] = ??? }
class R3 { def getClass(): Class[_] = ??? }
trait T1 { def getClass(): Class[_] = ??? }
// A value class's `getClass` must still be a `Class[_ <: AnyVal]`.
class V2(val x: Int) extends AnyVal { override def getClass(): Class[String] = ??? }
class V4(val x: Int) extends AnyVal { override def getClass(): String = ??? }
// A different parameter type is an overload, not an override, even against
// a Java method (only `Object` against `AnyRef` matches).
abstract class W1 extends ju.AbstractSet[String] { override def remove(elem: String): Boolean = false }
trait S { def f(x: Any): Int }
abstract class C1 extends S { override def f(x: AnyRef): Int = 1 }
// A member no base class and no self type has.
trait Sized { def size: Int }
trait G { this: Sized => override def length: Int = 1 }
