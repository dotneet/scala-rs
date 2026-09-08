// A member whose signature the first round has nothing to fix, in a template
// whose `import` the first round cannot resolve.
//
// `Component` imports through `holder`, an abstract `val` declared by the self
// type `Holder`, whose own signature pass has not run when the import is
// typed. The signature round therefore misses the import, and only a member
// with something *unresolved in its own signature* is built again on the round
// that has it. `tag(a: String, b: String)` has nothing unresolved -- `String`
// resolves with or without the import -- so it was never rebuilt, and the
// scope snapshot kept for its body was the one with no `import holder.api._`
// in it.
//
// `Use.go` is written above `Component`, so the body is typed by an on-demand
// completion standing in that snapshot: `mark` was "not a member of String",
// with the failure landing on the *definition*, not on the call. Both
// alternatives are here because the pair is what makes the selectivity
// visible -- and because slick's `byRepository(String, String)` /
// `byRepository(Rep[String], Rep[String])` is exactly this shape.
object Main {
  def main(args: Array[String]): Unit = {
    println(Use.go)
    println(Use.goBoxes)
    println(Use.viaSelf)
  }
}

object Use extends Holder with Component {
  val holder: Provider = new Provider
  object I extends Inner
  def go: String = I.tag("x", "y")
  def goBoxes: String = I.tag(new Box("p"), new Box("q"))
  // The nullary one takes its result type from the same snapshot.
  def viaSelf: String = I.both
}

trait Component { self: Holder =>
  import holder.api._

  trait Inner {
    def tag(a: String, b: String) = a.mark + b.mark
    def tag(a: Box, b: Box) = a.mark + b.mark
    def both = "a".mark + new Box("b").mark
  }
}

class Box(val s: String)

trait Api {
  implicit class Marked(s: String) { def mark: String = "[" + s + "]" }
  implicit class MarkedBox(b: Box) { def mark: String = "{" + b.s + "}" }
}

trait Holder { val holder: Provider }

class Provider { val api: Api = new Api {} }
