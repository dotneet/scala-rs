// A *universal trait* is not a value class. nsc leaves `Object` out of its
// base classes too, but then bans redefining `Object`'s final methods outright
// -- `RefChecks.checkAllOverrides` guards that ban with
// `clazz.isTrait && !clazz.isSubClass(AnyValClass)`, so extending `Any` buys a
// trait no exemption. scalac 2.13.16 rejects this with "trait cannot redefine
// final method from class AnyRef".
//
// Pins the boundary of the `AnyVal` exemption in `override_check::is_any_rooted`:
// `tests/fixtures/libanyval_overload.scala` shows the value class `Meters`
// writing exactly this and being accepted.
trait Univ extends Any {
  def notify(): String = "Univ.notify"
}
object Main {
  def main(args: Array[String]): Unit = println("unreachable")
}
