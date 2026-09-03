// agent/final3, part 1 of a pair. Must be passed to the compiler *before*
// `final3_def.scala`: both roots exercised here are signature-pass ordering
// bugs and disappear if the definitions are walked first.
package final3use

import final3def.{Interp, Store}

class Profile {
  // Root A: `Store.createEmpty` is declared in a file that comes later, so its
  // written result type is not known yet. Completing this `val` on demand
  // during the signature pass cached `<notype>` for it, and the nested class's
  // parent clause below then reported
  // "no matching overload for constructor Interp with arguments (<notype>, Any)".
  val emptyDB = Store.createEmpty

  // A nested template's parents are typed in the *enclosing* template's
  // signature phase, which is what forces `emptyDB` that early.
  class SubInterp(param: Any) extends Interp(emptyDB, param) {
    // Root B: this borrows its result type from `Interp.run(n: Int): Any`,
    // whose own signature the pass had not reached either -- so the method
    // stayed inference-bound and its self-call reported
    // "recursive method run needs result type".
    override def run(n: Int) = if (n <= 0) "done" else run(n - 1)
  }

  def go: String = new SubInterp("p").run(3).toString
}

object Main {
  def main(args: Array[String]): Unit = {
    val p = new Profile
    println(p.emptyDB.label)
    println(p.go)
  }
}
