// Compile-only: scalac 2.13.16 accepts these, but a value class that
// overrides `getClass` cannot be *loaded* (scalac's own output fails with
// `IncompatibleClassChangeError: class Meter overrides final method
// java.lang.Object.getClass()`), so there is nothing to run.
//
// A value class may override `Any.getClass(): Class[_]` -- nsc's
// `Any_getClass` is deferred, not `Object`'s final one -- as long as the
// result conforms to `AnyVal.getClass(): Class[_ <: AnyVal]`. scala/scala's
// `Int.scala` and the eight other primitive classes do exactly this.
class Meter(val v: Int) extends AnyVal {
  override def getClass(): Class[Meter] = classOf[Meter]
}
class Gram(val v: Int) extends AnyVal {
  override def getClass(): Class[_ <: Gram] = classOf[Gram]
}
