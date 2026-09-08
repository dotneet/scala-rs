// The gaps in the bridge, each named by its own diagnostic. `docs/macros.md`
// §7.11.
//
// An argument is handed to the implementation as the *syntax* the call site
// wrote and is typechecked again where the expansion lands, so only the shapes
// whose meaning survives that trip are sent; a type argument becomes a
// `WeakTypeTag` the engine builds from `mirror.staticClass`, so only a type
// that call can reach can be one. Everything else is refused **by name** -- a
// macro handed a tree that is not the one the programmer wrote would expand to
// a wrong program that still compiles.
//
// A type constructor *applied to type arguments* used to be refused here too.
// It is not any more: `docs/macros.md` §7.21 made the descriptor carry its
// arguments, and `tests/fixtures/gbm_use.scala` runs `List[Int]`,
// `Map[String, List[Int]]`, `(Int, String)`, `Int => String`, `Array[Int]` and
// `ClassTag[GbmRow]` against real scalac's own output. What is left here is a
// **singleton type**, which no `staticClass` call can rebuild.
//
// **Real scalac 2.13.16 compiles and runs this file** (it prints 2 / 2 / 2 /
// `Main.type`): every line here is valid Scala, and what is pinned is that
// scala-rs says which piece it is missing rather than guessing.
import scala.language.experimental.macros

object EgGaps {
  def plus1(x: Int): Int = macro EgImpl.plusImpl
  def nameOf[T]: String = macro EgImpl.nameOfImpl[T]
}

object Main {
  def main(args: Array[String]): Unit = {
    // A block argument: `scala-rs cannot hand a block to a macro
    // implementation yet`.
    println(EgGaps.plus1({ val a = 1; a }))
    // A function literal, through a higher-order call the macro takes: same
    // shape of refusal.
    println(EgGaps.plus1(List(1).map((n: Int) => n).head))
    println(EgGaps.plus1(((n: Int) => n)(1)))
    // A type argument no `staticClass` call can rebuild.
    println(EgGaps.nameOf[Main.type])
  }
}
