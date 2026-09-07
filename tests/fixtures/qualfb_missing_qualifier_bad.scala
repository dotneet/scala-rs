// Both sides of the discriminator in `qualifier_names_nothing`
// (`crates/typer/src/check_types.rs`): a *qualifier* that denotes nothing is
// reported as the missing value it is, while a qualifier that denotes
// something and merely lacks the member keeps its own message.
//
// `tree_to_type`'s `TreeKind::Select` arm falls back on the bare simple name
// when `lookup_qualified_type` finds nothing, because a path this pass cannot
// model still has to resolve that way. When the qualifier itself names
// nothing there is no such path, and the fallback used to answer with
// whatever happened to share the simple name -- which is how a missing
// `Apply.scala` turned every cats `trait AllOps extends Ops with
// Apply.AllOps` into a self-inheriting trait.
//
// Real scalac 2.13.16 on this file:
//
//   qualfb_missing_qualifier_bad.scala:31: error: not found: value Missing
//   qualfb_missing_qualifier_bad.scala:35: error: not found: value Absent
//   qualfb_missing_qualifier_bad.scala:39: error: type Nope is not a member of object Fmt
//   qualfb_missing_qualifier_bad.scala:44: error: not found: value Gone
//
// Line 31 used to be `illegal cyclic reference involving trait AllOps` and
// line 44 was accepted outright: `D` silently inherited the unrelated
// top-level `Real`.
object Fmt {
  trait Ops[A]
  type Out = String
  trait Real
}

object A {
  trait AllOps[X] extends Fmt.Ops[X] with Missing.AllOps[X]
}

object B {
  def f(x: Absent.Out): Int = 0
}

object C {
  def g(x: Fmt.Nope): Int = 0
}

trait Real

object D extends Gone.Real
