// The other neighbouring case: `new C()` for a binary *abstract class* (which
// does have a real `<init>` in its class file, unlike a trait) must keep
// working once a trait's missing `<init>` is repaired.
abstract class GbTraitBase {
  def greet(name: String): String
}
