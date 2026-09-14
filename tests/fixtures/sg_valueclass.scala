// A value class is erased in JVM descriptors but stays boxed in generic
// Signature type arguments.
package sgvc

trait Box[A]

class Wrapped(val value: String) extends AnyVal

class Holder extends Box[Wrapped]

class ArrayHolder extends Box[Array[Wrapped]]

class Api {
  def wrapped: Box[Wrapped] = null
  def direct(w: Wrapped): String = w.value
}
