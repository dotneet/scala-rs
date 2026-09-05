// A macro read from a jar's pickle is *expanded*, never assumed: when the
// expansion cannot be run, the call site is an error naming the reason.
//
// This is gitbucket's own shape and the honest limit of the JVM bridge. The
// implementation's `reify` rebuilds `E`'s type through a runtime mirror, which
// resolves a class **by name against the classpath** -- so a class this very
// run is compiling has no class file for it to find. `TqRow` (from
// `tq_mdef.scala`, already compiled) expands; `TqLocal` below cannot.
//
// Real scalac has no such limit -- it expands in its own universe, where the
// symbol exists -- so this is a diagnostic, not a claim about the program.
import tqm.api._

class TqLocal(tag: TqTag) {
  def label: String = "local" + tag.n
}

object Main {
  lazy val locals = TqQuery[TqLocal]

  def main(args: Array[String]): Unit = {
    println(locals.head.label)
  }
}
