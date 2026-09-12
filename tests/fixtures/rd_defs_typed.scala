// A `val` with a declared constructor type (`List[Int]`) and a local `def`
// with a type parameter of its own, inside `reify`. Both were refused by the
// `agent/reifydefs` slice, whose reifier had no structural type reifier;
// `reify` now walks the typed body and rebuilds a written type from what it
// resolved to, and a type parameter the body itself declares by name
// (`docs/notes/reify-design.md`). Real scalac 2.13.16 compiles this file, and
// so does scala-rs (`crates/cli/tests/reifydefs.rs`).
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
