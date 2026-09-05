// The trait half of the `ti_` interop checks. One compiler builds this file,
// the *other* builds `ti_app.scala` / `ti_stack.scala` against the class files
// it produced, and the pair has to behave like scalac-on-scalac.
// See `crates/cli/tests/traitclass.rs`.
package tilib

trait Counter {
  // nsc expands the name of an unqualified `private` value member of a trait
  // (`tilib$Counter$$n`), and names the mixin setter after the *expanded*
  // getter (`tilib$Counter$_setter_$tilib$Counter$$n_$eq`). `private[this]`
  // expands the same way; `private[tilib]` and `protected` do not.
  private val n: Int = 7
  private[this] val seed: String = "s"
  private var m: Int = 1
  // Declared but deliberately never *read* here. The expanded name is what
  // this file checks (`tilib$Counter$$doubled`); evaluating a trait `lazy val`
  // from a class real scalac compiled needs nsc's `d$` static beside a
  // `default` initialiser, which we do not emit yet for a `lazy val` of any
  // access -- a separate gap, recorded in `docs/notes/known-gaps-backlog.md`.
  private lazy val doubled: Int = n * 2
  private[tilib] val pkg: Int = 5
  protected val prot: Int = 6
  val plain: String = "p"
  var count: Int = 2

  def bump: Int = { m = m + n; m }
  def tick: Int = { count = count + 1; count }
  def base: Int = n
  def show: String = seed + "/" + pkg + "/" + prot + "/" + plain + "/" + count
}

trait Base { def label: String }

// `abstract override`: `super.label` is bound by the linearization of whatever
// concrete class mixes the trait in, so the class owes a
// `tilib$Loud$$super$label` accessor — which it only writes when the trait's
// *signature* declares the member with nsc's `SUPERACCESSOR` flag.
trait Loud extends Base {
  abstract override def label: String = "<" + super.label + ">"
}

trait Twice extends Base {
  abstract override def label: String = super.label + super.label
}
