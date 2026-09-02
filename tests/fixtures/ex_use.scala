// Def macros whose implementations return `c.Expr[T]` and read `c.prefix`.
// `docs/macros.md` §7.12. Compiled against `ex_impl.scala`'s class files, the
// way nsc requires -- the implementation has to come from an earlier run.
//
// Real scalac 2.13.16 compiles this file the same way against the same
// implementations, and `crates/cli/tests/engine.rs` compares the two
// programs' output line for line. A macro that expanded to something else
// would still compile and still run: only the output would differ.
import scala.language.experimental.macros

class ExTagged(val tag: String) {
  // `c.prefix`: the implementation names the receiver this was called on.
  def label: String = macro ExImpl.tagImpl

  // The prefix and an argument in the same expansion.
  def joined(other: ExRow): String = macro ExImpl.bothImpl

  // The prefix's tag, which nsc leaves as `Nothing`.
  def prefixType: String = macro ExImpl.prefixTypeImpl
}

object ExUse {
  def const(): Int = macro ExImpl.constImpl

  def double(x: Int): Int = macro ExImpl.doubleImpl

  // The shape of `slick.lifted.TableQuery.apply[E]`: no value arguments, one
  // type argument, and a result type that mentions it. The implementation
  // gets `E` as a `WeakTypeTag` and builds `new E(...)` out of its `tpe`.
  def box[E]: ExBox[E] = macro ExImpl.boxImpl[E]

  // What the materialised `WeakTypeTag[ExBox[E]]` says its type is.
  def boxType[E]: String = macro ExImpl.boxTypeImpl[E]
}

object Main {
  def main(args: Array[String]): Unit = {
    println(ExUse.const())
    val y = 20
    println(ExUse.double(y))
    println(ExUse.double(3) + 1)

    val t = new ExTagged("left")
    println(t.label)
    val r = new ExRow("right")
    println(t.joined(r))
    println(t.prefixType)

    // The expansion mentions neither `ExBox` nor `ExRow` here: both come out
    // of the implementation and are resolved in this file's scope.
    val b = ExUse.box[ExRow]
    println(b)
    println(b.row.tag)
    println(ExUse.boxType[ExRow])
    println(ExUse.boxType[String])
  }
}
