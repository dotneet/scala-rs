// `reify { … }` over blocks and over names that stand for a *member* of a
// static `object` rather than for the `object` itself (`docs/macros.md`
// §7.17).
//
// Every line prints `showRaw` of the reified tree, so what is compared with
// real scalac 2.13.16 is the tree itself and not just "it ran". That matters
// here: `Select(Ident(RfHelper), TermName("twice"))` and a bare
// `Ident(TermName("twice"))` both compile and both evaluate to the same thing
// at a call site that happens to have a `twice` in scope -- only the printed
// tree tells them apart, and only the first one is hygienic.
import scala.reflect.runtime.universe._

object RfHelper {
  def twice(i: Int): Int = i * 2
  val four = 4
}

import RfHelper._

object Main {
  // A block of statements. nsc builds `Block(List(init), last)`; the parser
  // folds `{ e }` down to `e`, so a one-element block never reaches here.
  val block = reify { println("a"); println("b") }
  // Three statements, the last one a value: the block's own type is the last
  // expression's, which is what `Expr.apply[T]` is instantiated at.
  val blockValue = reify { println("a"); println("b"); twice(3) }
  // `println` is `scala.Predef.println` by the time nsc's reifier sees it.
  val predef = reify { println("hello") }
  // A member reached through an `import`, applied and unapplied.
  val imported = reify { twice(21) }
  val importedValue = reify { four }
  // A member selected on the `object` itself: the qualifier is the static
  // `object`, which was already reified by symbol before this slice.
  val qualified = reify { RfHelper.twice(RfHelper.four) }
  // A block nested in an argument position.
  val nested = reify { twice({ println("x"); 5 }) }

  def main(args: Array[String]): Unit = {
    println(showRaw(block.tree))
    println(showRaw(blockValue.tree))
    println(showRaw(predef.tree))
    println(showRaw(imported.tree))
    println(showRaw(importedValue.tree))
    println(showRaw(qualified.tree))
    println(showRaw(nested.tree))
    // The `T` of the resulting `Expr[T]` comes from the whole body, so a
    // block's type is its last expression's.
    println(block.staticType.toString)
    println(blockValue.staticType.toString)
  }
}
