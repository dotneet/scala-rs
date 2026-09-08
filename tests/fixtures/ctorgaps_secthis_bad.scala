// A *secondary* constructor's default is evaluated before the instance
// exists, exactly like the primary's, so it cannot name anything the class
// declares. scalac 2.13.16 reports `not found: value f` at line 12.
//
// This is the miscompile that opening the secondary-constructor default
// unlocked: with the class's member scope left in place, `f` resolved to the
// *field* and the spliced tree read it off the caller's own `this` --
// `java.lang.ClassCastException: class Main$ cannot be cast to class T`, with
// no diagnostic at all. See `Typer::record_secondary_ctor_default_scope`.
class T(val v: String) {
  val f = "F"
  def this(n: Int, s: String = f) = this(s + n)
}
// The companion is what makes the position match nsc's: the getter is
// declared on it, so the default's body is typed where it was *written*.
// Without one there is no getter, the body is typed only where an omitted
// argument splices it, and the same error lands on the call site instead.
object T
