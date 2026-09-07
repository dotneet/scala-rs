// A deep, wide, diamond-shaped mixin hierarchy whose linearization is
// *observable at run time*: every trait prints its own name and then delegates
// to `super`, so the printed order is exactly the linearization (SLS 5.1.2).
//
// This exists because a termination guard in `crates/typer/src/lin.rs` that
// truncated a linearization instead of diverging would leave this program
// compiling cleanly while the printed chain silently lost a trait -- a wrong
// answer at run time, with no diagnostic. So this fixture is executed, not
// merely compiled, and `crates/cli/tests/linearization.rs` additionally
// recompiles this same source with real scalac 2.13.16 and compares the two
// programs' output directly. The expected output below came from scalac.
//
// `Wider` below is the shape SLS 5.1.2's `+:` gets right and a C3 merge does
// not: a shared ancestor reached at two different depths. It used to compile
// cleanly and print the wrong chain.
//
// A mixin an earlier parent already extends -- `class C extends L3 with L1`
// where `L3` already extends `L1` -- is linearized correctly by `lin.rs` but
// still mis-resolved when the class body says `super`; that is a separate,
// pre-existing defect (`docs/not-implemented.md`) and is not measured here.
trait L0 {
  def t: String = "L0"
}
trait L1 extends L0 {
  override def t: String = "L1 " + super.t
}
trait L2 extends L0 {
  override def t: String = "L2 " + super.t
}
trait L3 extends L1 with L2 {
  override def t: String = "L3 " + super.t
}
trait L4 extends L1 {
  override def t: String = "L4 " + super.t
}
trait L5 extends L3 with L4 {
  override def t: String = "L5 " + super.t
}
trait L6 extends L2 with L4 {
  override def t: String = "L6 " + super.t
}
trait L7 extends L5 with L6 {
  override def t: String = "L7 " + super.t
}
trait L8 extends L7 {
  override def t: String = "L8 " + super.t
}
trait L9 extends L8 {
  override def t: String = "L9 " + super.t
}

// A class parent under the whole stack, so the linearization has to place a
// real superclass as well as the traits.
class Root {
  def label: String = "Root"
}

// Ten trait levels over four diamonds, reached through a single mixin.
class Deep extends Root with L9 {
  override def t: String = "Deep " + super.t
  def show: String = label + ": " + t
}

// Two incomparable mixins that share `L4` and, through it, `L1` and `L0`:
// the C3 merge has to interleave two whole linearizations rather than
// concatenate them.
class Wide extends Root with L5 with L6 {
  override def t: String = "Wide " + super.t
}

// Two mixins that meet only at `L1`, so the merge has to hold `L3`'s branch
// together instead of interleaving it with `L4`'s.
class Three extends Root with L3 with L4 {
  override def t: String = "Three " + super.t
}

// `L5` and `L6` share `L4`, but `L5` reaches it through `L3` and `L6` reaches
// it directly, so the shared ancestor sits at two different depths. SLS 5.1.2
// folds `L(L5) +: L(L6) +: L(Root)`, which keeps `L3` immediately behind `L5`;
// a C3 merge with no free head has to guess, and guessed `L4` first.
class Wider extends Root with L6 with L5 {
  override def t: String = "Wider " + super.t
}

object Main {
  def main(args: Array[String]): Unit = {
    println(new Deep().show)
    println(new Wide().t)
    println(new Three().t)
    println(new Wider().t)
  }
}
