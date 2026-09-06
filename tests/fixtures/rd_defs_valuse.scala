// Call site for `rd_defs_valimpl.scala`, compiled against its class files the
// way nsc requires -- the implementation has to come from an earlier run.
//
// The macro `def` itself lives here, alongside its call site, rather than in
// the implementation file: scala-rs does not yet write `macro_impl` into its
// own pickle (`docs/macros.md` §7.17 "What this is worth, measured", item
// under "Of the remaining 16"), so a macro `def` compiled in an *earlier*
// round is read back in a later one as an ordinary method with no bytecode.
// That is a separate, already-recorded gap and not what this fixture is
// testing.
import scala.language.experimental.macros

object RdDefsUse {
  def m1: Int = macro RdDefsHelper.m1Impl
}

object Main {
  def main(args: Array[String]): Unit = {
    println(RdDefsUse.m1)
  }
}
