// Qualified type prefixes that *do* denote something, in every shape this
// compiler has to keep resolving.
//
// `tree_to_type`'s `TreeKind::Select` arm falls back on the bare simple name
// when `lookup_qualified_type` finds nothing, because a path this pass cannot
// model still has to resolve that way -- Twirl writes `HtmlFormat.Appendable`
// in the parents clause of every generated template and the pickled alias has
// no symbol to find. `qualifier_names_nothing` narrows that fallback to
// qualifiers that resolve; this fixture is the guard on the other side, so
// that narrowing cannot start rejecting the shapes the fallback exists for.
//
// Every prefix below is deliberately *shadowed* by a top-level type of the
// same simple name, so a prefix that were dropped and re-resolved by its bare
// name would bind the wrong one and the program would print the wrong string.

// The decoys. Each shares a simple name with a member reached through a
// prefix below; none of them is what any of those prefixes means.
trait Out { def label: String = "decoy Out" }
trait Cell { def label: String = "decoy Cell" }
trait Inner { def label: String = "decoy Inner" }
trait Appendable { def label: String = "decoy Appendable" }

object Fmt {
  // The Twirl shape at source scale: an alias declared in an object, named
  // through that object in a parents clause and in an ordinary signature.
  type Out = Real
  trait Real { def label: String = "Fmt.Out" }
}

object Holder {
  trait Inner { def label: String = "Holder.Inner" }
}

class Box {
  class Cell { def label: String = "Box#Cell" }
  type Appendable = Cell
}

package deep {
  object Nested {
    trait Inner { def label: String = "deep.Nested.Inner" }
  }
}

// A parents clause whose prefix is a plain object in the same file.
object ViaObject extends Fmt.Out
// A parents clause whose prefix is an object reached through a package.
object ViaPackage extends deep.Nested.Inner
// The same, spelled from the root.
object ViaRoot extends _root_.deep.Nested.Inner
// A parents clause whose prefix is an object brought in by an import.
import deep.Nested
object ViaImport extends Nested.Inner
// A parents clause whose prefix is an object nested in an object.
object Outer {
  object Mid {
    trait Leaf { def label: String = "Outer.Mid.Leaf" }
  }
}
object ViaNested extends Outer.Mid.Leaf

object Main {
  // Ordinary signatures, where `strict_type_names` is on for a file whose
  // scope this compiler can enumerate.
  def viaObject(x: Fmt.Out): String = x.label
  def viaHolder(x: Holder.Inner): String = x.label
  def viaPackage(x: deep.Nested.Inner): String = x.label
  def viaRoot(x: _root_.deep.Nested.Inner): String = x.label

  def main(args: Array[String]): Unit = {
    println(viaObject(ViaObject))
    println(viaPackage(ViaPackage))
    println(viaRoot(ViaRoot))
    println(viaHolder(new Holder.Inner {}))
    println(ViaImport.label)
    println(ViaNested.label)

    // A path-dependent prefix: the qualifier is a *term*, and the alias on it
    // is reached through the value, not through any symbol table owner.
    val b = new Box
    val c: b.Cell = new b.Cell
    val a: b.Appendable = c
    println(a.label)

    // The decoys are still reachable under their bare names, so the prints
    // above really did go through the prefixes.
    println(new Out {}.label)
    println(new Cell {}.label)
    println(new Inner {}.label)
    println(new Appendable {}.label)
  }
}
