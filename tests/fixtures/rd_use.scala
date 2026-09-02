// Call sites for `rd_impl.scala`, compiled against its class files the way
// nsc requires -- the implementation has to come from an earlier run.
//
// Both expansions arrive as an `Expr` built by a `TreeCreator`, which is the
// form every `reify` produces. Real scalac 2.13.16 compiles the same two
// files against each other and the two programs must print the same thing:
// a creator that resolved `RdHelper` in the wrong universe, or spliced the
// argument's tree without rebasing it, would still compile.
import scala.language.experimental.macros

object RdUse {
  def fortyTwo: Int = macro RdImpl.fortyTwo
  def twice(x: Int): Int = macro RdImpl.twice
  def probes: Boolean = macro RdImpl.probes
}

object Main {
  def main(args: Array[String]): Unit = {
    println(RdUse.fortyTwo)
    println(RdUse.twice(21))
    println(RdUse.probes)
  }
}
