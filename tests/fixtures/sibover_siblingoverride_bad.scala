// What `Check::drop_sibling_overrides` must still reject.
//
// Reducing a candidate set is only safe if it cannot swallow a real error, so
// both cases here are rejections real scalac 2.13.16 makes, checked line for
// line by `crates/cli/tests/sibover.rs`.
//
// Case 1: a genuine ambiguity between two sibling traits stays ambiguous.
// `HA.h` and `HB.h` have unrelated parameter types, so they are two members
// and not one however the linearization orders their owners -- `HBase.h`
// standing above both is not enough to make them the same member, which is
// exactly what `Check::same_member_at` is there to say. A `null` argument fits
// both, and the call has to stay ambiguous.
//
// Case 2: a name no sibling defines is still not a member. The reduction runs
// on a set that was found; it must never invent one.
//
// Not here, deliberately: `class Flip[+A] extends Wide[A] with CFac2[A, Flip]
// with CFac1[A, Wide]` -- two sibling overrides written so that the wider one
// heads the linearization. scalac rejects the *class* ("incompatible type in
// overriding", a refchecks error), and this compiler accepts it, both before
// and after this slice. That gap is in the override check, not in member
// lookup, and a fixture asserting the rejection would fail for a reason that
// has nothing to do with this rule.

class ArgA
class ArgB
trait HBase { def h(x: Any): String = "HBase" }
trait HA extends HBase { def h(x: ArgA): String = "HA" }
trait HB extends HBase { def h(x: ArgB): String = "HB" }
class HBoth extends HA with HB

object Main {
  def main(args: Array[String]): Unit = {
    val b = new HBoth
    println(b.h(null))
    println(b.nosuch)
  }
}
