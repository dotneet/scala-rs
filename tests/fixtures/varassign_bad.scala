// The assignments this compiler **accepted** at the branch point, each of
// which real scalac 2.13.16 rejects, and each of which was miscompiled rather
// than merely mistyped.
//
// `check_reassignment` bailed out on anything whose left side did not resolve
// to a `Term`, on the reasoning that a `Method` left side is an
// already-resolved `x_=` setter. A `def`, a `val` that arrives from a class
// file as a bare getter, and an `object` are all left sides that are not
// `Term` and not setters, so all three fell through the hole:
//
//   * `def v: Int = 1; v = 2` emitted `putfield C.v:I` for a field `C` does
//     not declare;
//   * `object O; O = null` emitted `putfield scala/runtime.O:LO$;`;
//   * `Nil.length = 2` did the same through a `Select`.
//
// scalac's own words, for the record:
//   value v_= is not a member of C
//   reassignment to val            (the `def` and the `object`)
//   value length_= is not a member of object Nil

class C {
  def v: Int = 1
  def f(): Unit = { v = 2 }
}

class Base { def w: Int = 1 }
class Derived extends Base

object O

object Bad {
  def localDef(): Unit = { def x = 1; x = 2 }
  def theObject(): Unit = { O = null }
  def jarMember(): Unit = { Nil.length = 2 }
  def inheritedDef(): Unit = { val d = new Derived; d.w = 2 }
}
