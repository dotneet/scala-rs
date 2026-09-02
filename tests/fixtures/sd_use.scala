// Def macros whose expansions contain `Function` and `ValDef`.
// `docs/macros.md` §7.13. Compiled against `sd_impl.scala`'s class files, the
// way nsc requires -- the implementation has to come from an earlier run.
//
// Real scalac 2.13.16 compiles this file the same way against the same
// implementations, and `crates/cli/tests/engine.rs` compares the two
// programs' output line for line. A macro that expanded to a *different*
// function literal would still compile and still run: only the output would
// differ.
import scala.language.experimental.macros

object SdUse {
  // slick's `TableQuery.apply[E]`: no value arguments, one type argument, and
  // an expansion that builds `tag => new E(tag)` out of the tag's type.
  def query[E]: SdQuery[E] = macro SdImpl.queryImpl[E]

  // A macro that takes no arguments and whose *result* is a function. The
  // argument list at `SdUse.adder(20, 22)` below belongs to the expansion,
  // not to the macro: the expander walks in to the node that carries the
  // macro def's own clauses (here, none).
  def adder: (Int, Int) => Int = macro SdImpl.adderImpl

  def twice(x: Int): Int = macro SdImpl.twiceImpl
}

object Main {
  def main(args: Array[String]): Unit = {
    // Neither `SdCoffees` nor its constructor is named in the implementation:
    // the expansion carries `new <E>(tag)` and this file's scope resolves it.
    println(SdUse.query[SdCoffees].head.label)
    println(SdUse.query[SdTeas].head.label)

    val f = SdUse.adder
    println(f(3, 4))
    // The macro is nullary and its *result* is applied: the argument list
    // belongs to the expansion, not to the macro.
    println(SdUse.adder(20, 22))

    var calls = 0
    def bump(): Int = { calls += 1; 5 }
    println(SdUse.twice(bump()))
    // The `val` in the expansion means the argument is evaluated once.
    println(calls)
  }
}
