// The half of the rule that says it is a restriction.
//
// A re-declaration that only *restates* the inherited definition is one
// member and takes the derived class's spelling
// (`tests/fixtures/libov_reabstract.scala`). One that says something
// *different* does not replace the definition: nsc's `findMember` stops
// replacing a member it has already found once either side is `DEFERRED`, so
// the concrete definition stays the member and its type is the one in force.
//
// Every line below is rejected by real scalac 2.13.16 at the line marked,
// with the message quoted. `bad1` and `bad2` are the two the rule decides:
// on the pre-fix binary both are rejected for the wrong reason, with the
// unreduced candidate set as the receiver (`<overload Ops | LinOps | Box>`
// and `<overload Any | NarrowBox>`). `bad3` and `bad4` are unchanged by the
// rule and are here to say so: a real overload set stays resolvable and a
// real ambiguity stays one.

// --- bad1: nsc reports `Ops`, not `Box`. scalac line 25:
//     value tag is not a member of Ops
trait Ops { def rest: Ops = throw new UnsupportedOperationException("Ops.rest") }
trait LinOps extends Ops { override def rest: LinOps }
abstract class Box extends LinOps {
  override def rest: Box
  def tag: String
  def bad1: String = rest.tag
}

// --- bad2: the definition's type is generic but the declaration narrows it
//     past the instantiation, so it does not restate it. scalac line 35:
//     value note is not a member of Any
trait Holder[+C] { def get: C = null.asInstanceOf[C] }
abstract class NarrowBox extends Holder[Any] {
  override def get: NarrowBox
  def note: String
  def bad2: String = get.note
}

// --- bad3: an inherited alternative that a subclass does not override stays
//     an alternative. `f(String)` is not `f(Int)`, so `f(true)` matches
//     neither. scalac line 45:
//     overloaded method f with alternatives ... cannot be applied to (Boolean)
trait Two { def f(x: Int): String = "Two" }
class Three extends Two {
  def f(x: String): String = "Three"
  def bad3: String = f(true)
}

// --- bad4: a real ambiguity has to stay one. Both alternatives take exactly
//     one `Null`-accepting reference parameter and neither is more specific.
//     scalac line 54: ambiguous reference to overloaded definition
class Amb {
  def g(x: String): String = "String"
  def g(x: java.lang.Integer): String = "Integer"
  def bad4: String = g(null)
}
