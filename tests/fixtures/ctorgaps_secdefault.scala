// A default argument on a *secondary* constructor.
//
// `synthesize_ctor_default_getters` used to run over the primary's parameters
// only, so `new Deft("abc")` reported `value <init>$default$2 is not a member
// of Main$`: the getter the call site wanted was never declared, and the
// lookup fell out to the enclosing object.
//
// Every case prints the value the default actually produced -- a getter that
// answers with the wrong expression is invisible to an error count.

// The reduction from `docs/not-implemented.md`.
class Deft(val p: Int, val q: String) {
  def this(p: String, q: String = "dq") = this(p.length, q)
}

// A companion, so the getters are really emitted (`crate::ctor_defaults` only
// declares them on a companion module that already exists) -- and a `case`
// class, so the synthetic `apply` is in the same class file. The secondary
// owes no `apply$default$n`: the case-class `apply` mirrors the primary.
case class Tagged(name: String, tag: String) {
  def this(name: String, n: Int = 3) = this(name, "n" + n)
}

// A secondary that delegates to *another secondary* rather than to the
// primary, and lets that one's default fill itself in. Only one constructor
// of a class may define defaults at all -- nsc rejects the rest with
// "multiple overloaded alternatives of constructor Chain define default
// arguments", which `ctorgaps_secdefault_bad.scala` pins -- so the delegating
// constructor takes none of its own.
class Chain(val v: String) {
  def this(n: Int, sep: String = "-") = this(sep + n + sep)
  def this(f: Boolean) = this(if (f) 1 else 0)
}

// The default is not a literal: it is computed where the constructor was
// written, and it reads a top-level object rather than anything the class
// declares (which is what nsc forbids, and what
// `ctorgaps_secthis_bad.scala` pins).
object Seed {
  val base: Int = 20
  def next(k: Int): Int = base + k
}
class Computed(val total: String) {
  def this(k: Int, bump: Int = Seed.next(2)) = this("t" + (k * 100 + bump))
}

// Found here, not fixed here: nsc fills a default only when no alternative
// applies *without* one, so `new Prefer(1)` on
// `class Prefer(n: Int) { def this(k: Int, bump: Int = 5) = … }` is the
// *primary*. Both alternatives are applicable to this compiler at once and it
// reports "ambiguous overload for constructor" -- pre-existing, reproduces
// unchanged on the branch point, and a rejection rather than a wrong pick.
// `crates/cli/tests/secondaryctor.rs` pins it.

// The primary and a secondary both take defaults *in different classes*, so
// the two synthesis paths are exercised side by side in one run.
class Both(val a: Int, val b: Int = 9) {
  def this(s: String) = this(s.length)
}
object Both

// A generic class: the getter repeats the class's type parameters and its
// result type is inferred, exactly as for a primary's. The arities differ so
// that the pick is decided before defaults are considered at all (see the
// `Prefer` note above).
class Boxed[A](val items: List[A], val label: String, val n: Int) {
  def this(one: A, label: String = "one") = this(one :: Nil, label, 1)
}
object Boxed

object Main {
  def main(args: Array[String]): Unit = {
    // The secondary's default, omitted and written.
    println(new Deft("abc").q)
    println(new Deft("abcd", "given").q)
    // The primary is still reachable and unchanged.
    println(new Deft(7, "prim").q)

    val t = new Tagged("x")
    println(t.name + "/" + t.tag)
    println(new Tagged("y", 8).tag)
    println(Tagged("z", "direct").tag)

    // Delegation: `new Chain(true)` -> `this(1)` -> `this(1, "-")` (the other
    // secondary's default) -> the primary `this("-1-")`.
    println(new Chain(5).v)
    println(new Chain(5, "*").v)
    println(new Chain(true).v)
    println(new Chain(false).v)

    println(new Computed(1).total)
    println(new Computed(1, 5).total)
    println(new Computed("raw").total)

    println("" + new Both("abcd").a + "/" + new Both("abcd").b)
    println(new Both(1).b)
    println(new Both(1, 2).b)

    val b1 = new Boxed(1)
    println(b1.items.mkString(",") + "@" + b1.label)
    val b2 = new Boxed(1, "two")
    println(b2.items.mkString(",") + "@" + b2.label)
    val b3 = new Boxed(4 :: 5 :: Nil, "three", 2)
    println(b3.items.mkString(",") + "@" + b3.label + "@" + b3.n)
  }
}
