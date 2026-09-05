// The trait half of the `bt2_` interop checks: what one compiler has to tell
// the *other* about a trait through a class file alone. One compiler builds
// this file, the other builds `bt2_app.scala` / `bt2_stack.scala` against the
// class files it produced, and the pair has to behave like scalac-on-scalac.
// See `crates/cli/tests/traitclass.rs`.
package bt2lib

trait Base { def label: String }

// `abstract override`, read back from a class file. The interface's `default
// label()` reaches the next layer through `bt2lib$Loud$$super$label`, which
// the *class* owes -- and a class method always beats an interface `default`,
// so a class that does not implement it runs the base body with no exception
// and no diagnostic.
trait Loud extends Base {
  abstract override def label: String = "<" + super.label + ">"
}

trait Twice extends Base {
  abstract override def label: String = super.label + super.label
}

trait Counter {
  var count: Int = 2
  // A trait `lazy val` is the one `val` whose accessor is concrete on the
  // interface: the initialiser is a `default` method with a `doubled$` static
  // beside it, and the implementing class's `doubled$lzycompute` calls that
  // static. Reading it is what fails when the interface only declares it.
  lazy val doubled: Int = count * 2
  private lazy val secret: Int = count + 100
  def tick: Int = { count = count + 1; count }
  def peek: Int = secret
}

// Declarations, not definitions. An interface spells a deferred member and a
// concrete one identically (`public abstract int dv()` either way), so only
// the pickle says which is which.
trait Decl {
  def m: String
  var dv: Int
  val cv: String = "c"
}
