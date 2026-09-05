// Calling a macro def that exists only in a *jar's pickle*: the shape
// gitbucket needs from slick, where `lazy val Issues = TableQuery[Issues]`
// means the companion's parameterless macro `apply[E]`.
//
// Two things are being pinned:
//
// 1. The macro def is supplied at all. A macro emits no bytecode, so the only
//    record of it is the pickle's `MACRO` flag plus the `@macroImpl`
//    annotation, which `PickleSupply::install_pickled_macro` now reads. Before
//    that, `object TqQuery`'s only visible `apply` was the one taking the
//    constructor function, and `TqQuery[TqRow]` came out as that method's
//    *un-applied* type -- `value head is not a member of ((TqTag) => TqRow)
//    TqQuery[TqRow]`.
//
// 2. The two `apply` alternatives are told apart by position, not by their
//    type arguments (both take one). SLS 6.26.3: in value position only the
//    parameterless alternative survives, and as the callee of an `Apply` only
//    the one that takes parameters does. Both spellings are below.
//
// Compiled by scala-rs against `tq_mdef.scala`'s class files, which real
// scalac wrote. The expansion runs the real implementation through the JVM
// bridge, so this also checks that a macro implementation read from a class
// file -- never from source -- is invoked with the right tags.
import tqm.{TqRow, TqOther}
import tqm.api._

object Main {
  // The macro alternative: no value arguments.
  lazy val rows = TqQuery[TqRow]
  lazy val others = TqQuery[TqOther]
  // The ordinary alternative, as the callee of an `Apply`.
  val explicit = TqQuery[TqRow](t => new TqRow(t))

  def main(args: Array[String]): Unit = {
    println(rows.head.label)
    println(others.head.label)
    println(rows.at(7).label)
    println(explicit.at(4).label)
    // The macro's result is a real `TqQuery[TqRow]`, so its members resolve.
    val q: TqQuery[TqRow] = rows
    println(q.head.label)
  }
}
