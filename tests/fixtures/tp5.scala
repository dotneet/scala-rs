// Same bug as `tp4`, but through a *Scala* generic superclass (not just a
// Java one) and through `object ... extends` (a module's own `<init>` takes
// no arguments, but the super constructor call it emits carries the
// arguments -- a separate code path from an ordinary class `<init>`). Every
// JVM primitive kind, since erasure boxes each one differently.
class Box[T](val value: T)

class ByteBox extends Box[Byte](1.toByte)
class ShortBox extends Box[Short](2.toShort)
class CharBox extends Box[Char]('y')
class IntBox extends Box[Int](3)
class LongBox extends Box[Long](123456789012L)
class FloatBox extends Box[Float](5.5f)
class DoubleBox extends Box[Double](6.5)
class BoolBox extends Box[Boolean](true)

object SingletonBox extends Box[Int](99)

object Main {
  def main(args: Array[String]): Unit = {
    println(new ByteBox().value)
    println(new ShortBox().value)
    println(new CharBox().value)
    println(new IntBox().value)
    println(new LongBox().value)
    println(new FloatBox().value)
    println(new DoubleBox().value)
    println(new BoolBox().value)
    println(SingletonBox.value)
  }
}
