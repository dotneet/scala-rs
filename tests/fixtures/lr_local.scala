// Method-local `lazy val`: the initialiser must not run at the declaration,
// must run at most once, and must see whatever the enclosing method has
// captured. Every case here was diffed against real scalac 2.13.16.
object Main {
  def never(n: Int): Int = {
    lazy val a: Int = { println("never-forced"); n + 1 }
    if (n > 100) a else 0
  }

  def once(n: Int): Int = {
    lazy val a: Int = { println("once-init"); n * 2 }
    a
  }

  // Three reads, one initialisation.
  def thrice(n: Int): Int = {
    lazy val a: Int = { println("thrice-init"); n + n }
    a + a + a
  }

  // Captures a local `val` and a local `var`. The `var` is read when the value
  // is first forced, not when it is declared.
  def captures(n: Int): String = {
    val base = "b" + n
    var bump = 1
    lazy val s: String = { println("captures-init"); base + "/" + bump }
    bump = 41
    s + "|" + s
  }

  // A `lazy val` reading one declared after it, and one declared before it.
  def deps(): Int = {
    lazy val a: Int = b + 1
    lazy val b: Int = { println("deps-b"); 2 }
    lazy val c: Int = b * 10
    c + a + b
  }

  // Every primitive cell class plus a reference one.
  def prims(): String = {
    lazy val z: Boolean = { println("z"); true }
    lazy val by: Byte = { println("by"); 7.toByte }
    lazy val ch: Char = { println("ch"); 'q' }
    lazy val sh: Short = { println("sh"); 9.toShort }
    lazy val i: Int = { println("i"); 11 }
    lazy val l: Long = { println("l"); 12L }
    lazy val f: Float = { println("f"); 1.5f }
    lazy val d: Double = { println("d"); 2.5 }
    lazy val s: String = { println("s"); "str" }
    lazy val xs: List[Int] = { println("xs"); 1 :: 2 :: Nil }
    // `xs` is read through its elements: the private runtime's `List`
    // does not print like the library's, and that is a different gap.
    "" + z + by + ch + sh + i + l + f + d + s + xs.head + xs.tail.head
  }

  // `Unit` has its own cell class (`LazyUnit`): only the flag, no value.
  def unit(): Int = {
    var count = 0
    lazy val u: Unit = { count += 1; println("unit-init") }
    u
    u
    u
    count
  }

  // A fresh cell per iteration: the initialiser runs once per loop pass.
  def loop(): Int = {
    var total = 0
    var i = 0
    while (i < 3) {
      lazy val a: Int = { println("loop-init " + i); i * 10 }
      total = total + a + a
      i = i + 1
    }
    total
  }

  // Inside a lambda, and captured *by* a lambda from the enclosing method.
  def lambdas(): String = {
    val f = (n: Int) => {
      lazy val a: String = { println("lambda-init " + n); "L" + n }
      a + a
    }
    lazy val outer: String = { println("outer-init"); "O" }
    val g = () => outer + outer
    f(1) + f(2) + g() + g()
  }

  // A failing initialiser leaves the cell uninitialised, so the next read
  // retries it -- `_initialized` is only set once the value is stored.
  def retry(): String = {
    var attempt = 0
    lazy val a: String = {
      attempt += 1
      if (attempt < 3) throw new RuntimeException("boom " + attempt)
      "ok after " + attempt
    }
    var out = ""
    var k = 0
    while (k < 4) {
      out = out + (try a
      catch { case e: RuntimeException => e.getMessage }) + ";"
      k = k + 1
    }
    out
  }

  // Nested `def`s of their own, so the accessor is lifted out of one.
  def nested(n: Int): Int = {
    def inner(m: Int): Int = {
      lazy val a: Int = { println("nested-init " + m); m + n }
      a + a
    }
    inner(1) + inner(2)
  }

  // No result type written: the cell class comes from the inferred type.
  def inferred(n: Int): String = {
    lazy val a = { println("inf-a"); n * 2 }
    lazy val s = { println("inf-s"); "v" + a }
    s + s + a
  }

  // A method type parameter erases to `Object`, so the cell is a `LazyRef`.
  def generic[A](a: A): String = {
    lazy val v: A = { println("gen"); a }
    "" + v + v
  }

  def main(args: Array[String]): Unit = {
    println(never(1))
    println(once(3))
    println(thrice(4))
    println(captures(5))
    println(deps())
    println(prims())
    println(unit())
    println(loop())
    println(lambdas())
    println(retry())
    println(nested(10))
    println(inferred(3))
    println(generic("x"))
    println(generic(7))
  }
}
