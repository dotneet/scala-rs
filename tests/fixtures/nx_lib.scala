// The library half of the separate-compilation ABI check in
// `crates/cli/tests/nullcross.rs`. One compiler builds this file, the other
// builds `nx_app.scala` against the class files, and the pair has to run.
//
// Everything here is a member whose *reachability* differs between the two
// compilers rather than a member either of them gets wrong on its own:
//
//   * `val n` / `val q` — scalac makes the backing field `private` and
//     publishes an accessor beside it, so a cross-unit read has to call.
//   * `var c` — likewise, through `c_$eq`.
//   * `private[this] val hidden` — has no accessor at all, in either
//     compiler, and is the control that says the rule is read off the class
//     file rather than applied to every field.
//   * `Store` — the same two shapes on an `object`.
//   * `NullSig` — `Null` erases to `scala/runtime/Null$`, not to `Object`.
package nxlib

class Holder(val n: Int, private[this] val hidden: Int) {
  val q: String = "q" + n
  var c: Int = 0
  def bump: Int = hidden + c
}

class Sub extends Holder(7, 1)

object Store {
  val greeting: String = "hi"
  var count: Int = 0
}

class NullSig {
  def n: Null = null
  def take(x: Null): Int = if (x == null) 1 else 2
  def ln: List[Null] = Nil
}
