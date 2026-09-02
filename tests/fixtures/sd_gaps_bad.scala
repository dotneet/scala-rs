// Stage-D forms the bridge deliberately does not carry, each named rather
// than guessed. `docs/macros.md` §7.13.
//
// The first is a shape real scalac 2.13.16 compiles and scala-rs does not;
// the second is one both compilers refuse, for different reasons -- the point
// here is that scala-rs refuses it *by name* instead of rebuilding a
// definition with a modifier silently dropped.
import scala.language.experimental.macros

class SdLocalRow(tag: SdTag) {
  def label: String = "local@" + tag.name
}

object SdGaps {
  def query[E]: SdQuery[E] = macro SdImpl.queryImpl[E]
  def deferred: Int = macro SdImpl.deferredImpl
}

object Main {
  def main(args: Array[String]): Unit = {
    // 1. The row class is compiled by *this* run, so the engine's mirror
    //    cannot find it: the macro classpath holds what an earlier run wrote,
    //    and `staticClass("SdLocalRow")` is how the tag for `E` is built.
    println(SdGaps.query[SdLocalRow].head.label)

    // 2. A `ValDef` in the expansion carrying a modifier with no name in the
    //    table.
    println(SdGaps.deferred)
  }
}
