// A constructor default in a *later* parameter clause.
//
// `new Curr(7)()` used to emit an `invokespecial` with one argument for a
// three-parameter descriptor -- `VerifyError: Bad type on operand stack` --
// because a `new`'s arguments reach `fill_defaults_and_implicits` already
// flattened while that function re-read the callee's *unflattened* `paramss`
// off the symbol, so the second clause was never seen as short.
//
// This is the only shape in which a constructor default may legally name an
// earlier parameter: nsc rejects a same-clause reference outright. So every
// line below prints the value the default actually produced -- a getter that
// answers with the wrong expression is invisible to an error count -- and the
// expected output is what scalac 2.13.16 prints compiling this same file.

object Src {
  val tag: String = "src"
}

// The primary, second clause, both defaults, both naming the first clause's
// parameter. No companion is written: the getter takes `a`, so it cannot be
// spliced at the call site and nsc synthesizes a `Curr$` to hold it.
class Curr(val a: Int, val b: String) {
  def this(a: Int)(b: String = "b" + a, c: Int = a * 2) = this(a, b + c)
}

// A default in a later clause that reads no earlier parameter at all: nsc
// emits a *nullary* getter for it, and passing it arguments would emit a call
// that cannot link.
class Plain(val a: Int)(val s: String = "flat")

// A later-clause default beside one the caller writes, so the fill starts
// part-way through the clause.
class Half(val a: Int)(val x: String = "x" + a, val y: String = "y" + a)

// A companion the source wrote already: the getters join it rather than
// creating a second module.
class Owned(val n: Int)(val label: String = "n=" + n)
object Owned {
  def zero: Owned = new Owned(0)()
}

// A generic class, whose getter repeats the class's type parameter and has
// its result type inferred.
class Box[T](val head: T)(val note: String = "box")

// Three clauses, with the default in the last one naming a parameter of the
// *first* -- not merely of the clause before it.
//
// The primary is `(Int, String)`, which is exactly what
// `new Three(2)("m")()` folds down to, so this is also where the fold and the
// pick have to agree: nsc selects the constructor on the *first* clause, and
// `(2)` alone is not a `(Int, String)`. Held to the alternatives whose first
// clause is one parameter long, the flat list can only be the secondary's.
class Three(val a: Int, val bc: String) {
  def this(a: Int)(m: String)(tail: String = m + a) = this(a, m + "/" + tail)
}

// nsc fills a default only when no alternative applies *without* one, so
// `new Prefer(1)` is the primary and prints `1`, not the secondary's
// `1 * 100 + 5`. Both are applicable to one `Int` at once, and weighing them
// together was `ambiguous overload for constructor`.
class Prefer(val n: Int) {
  def this(k: Int, bump: Int = 5) = this(k * 100 + bump)
}

// A later-clause default computed from a top-level object rather than a
// literal, so the getter's body is a real expression.
class Tagged(val a: Int)(val t: String = Src.tag + a)

object Main {
  def main(args: Array[String]): Unit = {
    // Both defaults filled, both reading `a`.
    println(new Curr(7)().b)
    // The second clause written in full, so nothing is filled.
    println(new Curr(7)("w", 1).b)
    // Nullary getter.
    println(new Plain(1)().s)
    println(new Plain(1)("said").s)
    // Part of the clause written, the rest filled.
    val h = new Half(3)()
    println(h.x + h.y)
    val h2 = new Half(3)("X")
    println(h2.x + h2.y)
    // The getter on a companion the source wrote.
    println(Owned.zero.label)
    println(new Owned(5)().label)
    // Inferred getter result on a generic class.
    println(new Box[Int](1)().note)
    println(new Box("s")().note)
    // Third clause naming the first clause's parameter.
    println(new Three(2)("m")().bc)
    println(new Three(2)("m")("z").bc)
    // ... and the primary of that same class, written in one clause.
    println(new Three(2, "z").bc)
    // The alternative that needs no default is preferred.
    println(new Prefer(1).n)
    println(new Prefer(1, 2).n)
    // A computed default.
    println(new Tagged(9)().t)
  }
}
