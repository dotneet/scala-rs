// The five bodies `rf_bad.scala` once refused, reified now that `reify`
// walks the typed body by symbol (`docs/notes/reify-design.md`). Each is
// printed the way real scalac 2.13.16 prints it (`crates/cli/tests/
// rf_reify.rs` runs the same file through both compilers).
import scala.reflect.runtime.universe._

object Main {
  val member = 1

  // 1. A member of the *enclosing* `object`: nsc's typer spells this
  //    `Main.this.member` and its reifier builds `mkThis(staticModule("Main")
  //    .asModule.moduleClass)`.
  val enclosing = reify { member }

  // 2. A pattern `val` definition inside a block, reified in its desugared
  //    form. nsc's desugaring spells its synthetic names differently, so the
  //    tree is not compared, only its type.
  val localDef = reify { val (a, b) = (1, 2); a + b }

  // 3. A `scala.math` package-object function, resolved through
  //    `staticModule("scala.math.package")`.
  val packageObject = reify { math.max(1, 2) }

  // 4. A `class` definition inside a block, and a `new` of it.
  val classDef = reify { class C { def f = 1 }; new C().f }

  def local(): Expr[Int] = {
    val here = 3
    // 5. A local of the enclosing method -- a free term.
    reify { here }
  }

  def main(args: Array[String]): Unit = {
    println(showRaw(enclosing.tree))
    println(localDef.staticType)
    println(showRaw(packageObject.tree))
    println(classDef.staticType)
    println(showRaw(local().tree))
  }
}
