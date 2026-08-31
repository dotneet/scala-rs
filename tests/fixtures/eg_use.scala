// Def macros that are actually *expanded* -- the JVM bridge of
// `docs/macros.md` §2.2, phase 2. Compiled against `eg_impl.scala`'s class
// files, the way nsc requires (the implementation must come from an earlier
// compilation run, because expanding means loading and running it).
//
// Real scalac 2.13.16 compiles this file the same way against the same
// implementations, and `crates/cli/tests/engine.rs` compares the two programs'
// output line for line.
import scala.language.experimental.macros

object EgUse {
  // No arguments: the whole expansion comes from the implementation.
  def const(): Int = macro EgImpl.constImpl

  // One argument, spliced into the expansion by the implementation.
  def plus1(x: Int): Int = macro EgImpl.plusImpl

  // The implementation takes a raw `Tree` rather than an `Expr`.
  def twice(x: Int): Int = macro EgImpl.twiceImpl

  // A type argument, which reaches the implementation as a `WeakTypeTag`.
  def nameOf[T]: String = macro EgImpl.nameOfImpl[T]

  // The expansion names `EgHelper`, which this file never imports: the tree
  // is resolved in the *call site's* scope, like nsc's.
  def label(): String = macro EgImpl.labelImpl
}

object Main {
  def main(args: Array[String]): Unit = {
    println(EgUse.const())
    val y = 41
    println(EgUse.plus1(y))
    println(EgUse.plus1(1))
    println(EgUse.twice(21))
    println(EgUse.nameOf[String])
    println(EgUse.nameOf[Int])
    println(EgUse.label())
    // The expansion is an ordinary expression afterwards: it nests, and the
    // arithmetic around it is folded as usual.
    println(EgUse.plus1(EgUse.const()) * 2)
  }
}
