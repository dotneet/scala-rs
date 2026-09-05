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
    //    cannot find it. That used to be a refusal; it now expands, because
    //    such a type travels as a placeholder symbol carrying its name
    //    (`docs/macros.md` §5.1). Kept here as the case that no longer is one.
    println(SdGaps.query[SdLocalRow].head.label)

    // 2. A `ValDef` in the expansion carrying a modifier with no name in the
    //    table.
    println(SdGaps.deferred)
  }
}
