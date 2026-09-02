// `recv.copy(...)` on a case class value whose class is reached only
// through inheritance/a method signature, never imported by *simple name*
// into the calling file's own scope.
//
// `try_rewrite_case_copy` rebuilds `recv.copy(f = v, ...)` as
// `new C(...)`, and built that `new`'s type head as a bare
// `Tree::dummy(TreeKind::Ident { name: cls_name })` -- relying on ordinary
// *lexical name* resolution to find `C` again when the rebuilt tree was
// typed, even though the caller already had `C`'s real `SymbolId` in hand
// (`class_sym_of` on the receiver's own type, not name lookup). A class
// reachable only through another file's inheritance chain -- never
// directly imported by the file doing the `.copy()` -- has no reason to
// have its simple name in scope there at all, and this reported "not found:
// type C" with no line/column (the synthesized tree carries no real span).
// slick's `slick.jdbc.BaseResultConverter`, which calls
// `super.getDumpInfo.copy(...)` without ever importing
// `slick.util.DumpInfo` itself, hit exactly this.
//
// The fix: when the `New` being typed already carries a resolved `sym` on
// its `tpt` (as this rebuild now sets, from the `SymbolId` it already
// determined), use it directly instead of re-resolving by name.
package t5cpa {
  final case class Item(name: String, tag: Int = 0)
}

package t5cpb {
  object Helper {
    // `t5cpa.Item` is never imported here by simple name -- only reached
    // through the `i: t5cpa.Item` parameter type.
    def retag(i: t5cpa.Item): t5cpa.Item = i.copy(tag = 9)
  }
}

object Main {
  def main(args: Array[String]): Unit = {
    val i = t5cpa.Item("widget")
    val r = t5cpb.Helper.retag(i)
    println(r.name)
    println(r.tag)
  }
}
