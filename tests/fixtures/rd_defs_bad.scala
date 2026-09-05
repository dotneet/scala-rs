// Two shapes of `val` / `def` reification still refused after the
// `agent/reifydefs` slice. **Real scalac 2.13.16 compiles this file** -- it
// is a confession of what is not implemented, not a program that ought to be
// rejected.
//
// A `val` / `def` bound *inside* a reify body is now reified structurally,
// by name (`docs/macros.md` §7.17); what remains unsolved is *its declared
// type*, when that type is not a single monomorphic class. Measured with
// `-Ymacro-debug-lite`, nsc rebuilds a written value type the same
// structural way it rebuilds the rest of the body (`AppliedTypeTree`,
// `Select`, ...), resolving only the leaf that names a class or a module by
// symbol; scala-rs has no such structural type reifier yet (`Reifier::typ`'s
// ordinary, non-reify branch is what a quasiquote uses for exactly this),
// so only the single-leaf case is built and anything with its own structure
// is refused rather than guessed at.
import scala.reflect.runtime.universe._

object Main {
  // 1. A type constructor applied to arguments (`List[Int]`). nsc reifies it
  //    as `AppliedTypeTree(Select(mkIdent(staticModule("scala.package")),
  //    TypeName("List")), List(mkIdent(staticClass("scala.Int"))))` --
  //    `List` is a member of the `scala` package object, not a class of its
  //    own, and the application needs a structural reifier this module does
  //    not have.
  val listVal = reify { val xs: List[Int] = List(1, 2, 3); xs.size }

  // 2. A locally declared `def`'s own type parameter, used in a value
  //    position. There is no tag to look it up by -- it is not the *macro
  //    implementation's* type parameter (`docs/macros.md` §7.16's `noTag`),
  //    it is one this very `def` introduces -- so it can only be built
  //    structurally too, the same gap as case 1.
  val genericDef = reify { def id[U](x: U): U = x; id(1) }

  def main(args: Array[String]): Unit = {
    println(listVal); println(genericDef)
  }
}
