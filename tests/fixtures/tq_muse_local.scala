// A macro whose type argument is a class **this run is compiling**.
//
// This used to be the honest limit of the JVM bridge and the file was called
// `tq_muse_bad.scala`: the implementation's `reify` rebuilds `E`'s type through
// a runtime mirror, which resolves a class by name against the classpath, so a
// class with no class file yet could not be reached. gitbucket's
// `lazy val Issues = TableQuery[Issues]` is that shape.
//
// It now expands. The type travels as a *placeholder* symbol carrying the
// class's full name and no info, and scala-rs recognises that name in the tree
// that comes back (`docs/macros.md` §5.1, `crates/typer/src/expand.rs`).
// `tests/fixtures/mg_use.scala` is the dual run against real scalac that says
// the expansion is the right one; this file pins that the shape compiles at
// all. It is not run here because `TqLocal.label` reads `tag.n`, a constructor
// `val` of a class in another compilation unit, and that is a separate
// code-generation bug unrelated to macros.
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
