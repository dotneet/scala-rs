// The shapes `reify { … }` still refuses after `docs/macros.md` §7.17.
//
// **Real scalac 2.13.16 compiles this file.** It is a confession of what is
// not implemented, not a program that ought to be rejected: each `reify`
// below is diagnosed by name rather than reified as the name that was
// written, because building the bare name would compile, run, and mean
// whatever stood at the expansion site.
import scala.reflect.runtime.universe._

object Main {
  val member = 1

  // 1. A member of the *enclosing* `object`. nsc's typer spells this
  //    `Main.this.member` and its reifier builds `mkThis(staticModule("Main")
  //    .asModule.moduleClass)`, a different tree from the `mkIdent` form this
  //    slice builds for an imported member.
  val enclosing = reify { member }

  // 2. A `val` definition inside a block. nsc reifies the bound symbol with
  //    `build.newNestedSymbol` and links every reference to it.
  val localDef = reify { val x = 1; x }

  // 3. A `scala.math` package-object function. nsc resolves it through
  //    `staticModule("scala.math.package")`; scala-rs models these members on
  //    the *package* rather than on a module class, so the owner is not a
  //    static `object` here.
  val packageObject = reify { math.max(1, 2) }

  // 4. A `class` definition inside a block, which needs the same nested
  //    symbols as case 2 plus a whole reified template.
  val classDef = reify { class C { def f = 1 }; new C().f }

  def local(): Expr[Int] = {
    val here = 3
    // 5. A local of the enclosing method -- nsc's free terms.
    reify { here }
  }

  def main(args: Array[String]): Unit = {
    println(enclosing); println(localDef); println(packageObject)
    println(classDef); println(local())
  }
}
