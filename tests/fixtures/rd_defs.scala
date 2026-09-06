// `reify { … }` over `val` and `def` definitions bound *inside* the body
// itself (the `agent/reifydefs` slice).
//
// `docs/macros.md` §7.17 refused every definition in a reify body outright,
// reasoning from nsc's *free-term* machinery (`build.newNestedSymbol`) --
// the shape needed for a local or a parameter bound *outside* the body.
// Measured with `-Ymacro-debug-lite`, a `val` / `def` the body binds *for
// itself* needs none of that: nsc reifies both the binding and every
// reference to it structurally, by name, the same way a quasiquote would.
// `println(showRaw(...))` is what tells the two apart -- a `val`/`def`
// reified as a bare name still compiles and runs, so only the printed tree
// says whether it was rebuilt as nsc's own reifier would, or merely as
// something that happens to work.
import scala.reflect.runtime.universe._

object Main {
  // An untyped `val`, referenced once after its own definition.
  val simpleVal = reify { val x = 1; x + 1 }
  // A `val` with an explicit type: `Int` is a monomorphic class, reified as
  // `mkIdent($m.staticClass("scala.Int"))` -- *not* wrapped in `mkTypeTree`,
  // unlike a type *argument* at a call site (`docs/macros.md` §7.15/7.16).
  val typedVal = reify { val x: Int = 1; x + 1 }
  // A `def` with a parameter, whose type is reified the same way.
  val simpleDef = reify { def f(y: Int): Int = y + 1; f(41) }
  // A `def` that calls itself: legal because nsc lets a block's `def`s see
  // each other (and themselves) regardless of textual order.
  val recDef = reify {
    def fact(n: Int): Int = if (n == 0) 1 else n * fact(n - 1)
    fact(5)
  }
  // Two `def`s that call each other -- forward reference to a `def` that
  // has not been reified yet, by name.
  val mutualDef = reify {
    def isEven(n: Int): Boolean = if (n == 0) true else isOdd(n - 1)
    def isOdd(n: Int): Boolean = if (n == 0) false else isEven(n - 1)
    isEven(10)
  }
  // A `val` and a `def` together, the `def` reading the `val`.
  val mixedDef = reify { val x: Int = 10; def f(y: Int): Int = y * 2; f(x) }

  def main(args: Array[String]): Unit = {
    println(showRaw(simpleVal.tree))
    println(showRaw(typedVal.tree))
    println(showRaw(simpleDef.tree))
    println(showRaw(recDef.tree))
    println(showRaw(mutualDef.tree))
    println(showRaw(mixedDef.tree))
  }
}
