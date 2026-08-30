// Shapes a local `lazy val` has to survive besides a plain method body: read
// from a local class, a `return` out of the enclosing method, a value-class
// result (erasure narrows the accessor to `int` while the cell stays a
// `LazyRef`), a `match` case body and a `try` block, and one initialiser
// reading a `lazy val` from an enclosing block.
class Meters(val v: Int) extends AnyVal

trait Named {
  def base: Int
  // A `lazy val` in a trait method: the accessor is hoisted onto the trait,
  // so its body is emitted through the trait's static path too.
  def viaTrait(n: Int): Int = {
    lazy val a: Int = { println("trait-lazy"); base + n }
    a + a
  }
}

class Holder(val base: Int) extends Named {
  // In a template statement (part of the constructor), and reading `this`.
  val fromCtor: Int = {
    lazy val a: Int = { println("ctor-lazy"); base * 2 }
    a + a
  }
  def readsThis(): String = {
    lazy val s: String = { println("this-lazy"); "b" + base + this.base }
    s + s
  }
}

object Main {
  // A local class reading a local lazy val.
  def viaClass(n: Int): Int = {
    lazy val x: Int = { println("viaClass-init"); n + 1 }
    class C { def g: Int = x + x }
    new C().g
  }

  // Non-local return out of a lazy initialiser.
  def ret(n: Int): Int = {
    lazy val x: Int = { if (n > 0) return 99; 1 }
    x + 1
  }

  // A value class result: erasure turns the accessor's return into `int`
  // while the cell stays a LazyRef.
  def valClass(n: Int): Int = {
    lazy val m: Meters = { println("vc-init"); new Meters(n * 3) }
    m.v + m.v
  }

  // In a `match` case body and in a `try` block.
  def inBlocks(n: Int): String = {
    val a = n match {
      case 1 =>
        lazy val s: String = { println("case-init"); "one" }
        s + s
      case _ => "other"
    }
    val b =
      try {
        lazy val s: String = { println("try-init"); "t" }
        s + s
      } finally ()
    a + b
  }

  // A lazy val whose initialiser reads a lazy val declared in an outer block.
  def nestedBlocks(n: Int): Int = {
    lazy val outer: Int = { println("outer"); n }
    val r = {
      lazy val inner: Int = { println("inner"); outer * 2 }
      inner + inner
    }
    r + outer
  }

  // Two `lazy val`s of the same name in sibling scopes.
  def sameName(n: Int): Int = {
    val a = { lazy val v: Int = { println("v1"); n }; v }
    val b = { lazy val v: Int = { println("v2"); n * 2 }; v }
    a + b
  }

  def main(args: Array[String]): Unit = {
    println(viaClass(1))
    println(ret(1))
    println(ret(-1))
    println(valClass(2))
    println(inBlocks(1))
    println(inBlocks(2))
    println(nestedBlocks(5))
    val h = new Holder(3)
    println(h.fromCtor)
    println(h.readsThis())
    println(h.viaTrait(4))
    println(sameName(5))
  }
}
