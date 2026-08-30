// The shapes a template-body statement actually takes in real code: an early
// `require` / `assert` validating the constructor arguments, `if` / `match` /
// `try` / `while` in statement position, a lambda, and the same statements in
// a `case class`, in a local class, in an anonymous class and in a member
// `object` reached through `$outer`.
object Main {
  var log: String = ""
  def note(s: String): Unit = { log = log + s + ";" }

  class Validated(val n: Int) {
    require(n > 0, "n must be positive")
    assert(n < 100)
    note("n" + n)
    private val doubled = n * 2
    if (doubled > 10) note("big") else note("small")
    n match {
      case 1 => note("one")
      case _ => note("many")
    }
    try { note("q" + (100 / n)) }
    catch { case _: ArithmeticException => note("boom") }
    var acc = 0
    var i = 1
    while (i <= 3) { acc = acc + i; i = i + 1 }
    note("acc" + acc)
    private val f: Int => Int = x => x + n
    note("f" + f(1))
  }

  class Sub extends Validated(2) {
    note("sub" + n)
  }

  case class Pt(x: Int, y: Int) {
    note("pt" + x + "," + y)
    val norm = x * x + y * y
    note("norm" + norm)
  }

  trait Greeter {
    def greet: String
    note("greeter")
  }

  class Outer {
    val tag = "outer"
    note("outer-stat")
    object Inner {
      note("inner:" + tag)
      val v = 42
      note("inner-v" + v)
    }
  }

  def main(args: Array[String]): Unit = {
    new Validated(3)
    println(log)

    log = ""
    new Sub
    println(log)

    log = ""
    println(Pt(3, 4))
    println(log)

    log = ""
    val g = new Greeter { def greet = "hi" }
    println(g.greet)
    println(log)

    log = ""
    val a = new AnyRef { note("anon") }
    println(a != null)
    println(log)

    log = ""
    class Local(k: Int) {
      note("local" + k)
      val z = k * 2
      note("localz" + z)
    }
    new Local(7)
    println(log)

    log = ""
    val o = new Outer
    println(o.Inner.v)
    println(o.Inner.v)
    println(log)

    log = ""
    // `require` throws before any of the body below it runs, so `log` stays
    // empty. (The private runtime's `require` does not prefix the message with
    // "requirement failed: ", so only the fact that it threw is compared.)
    try { new Validated(-1) }
    catch { case _: IllegalArgumentException => println("caught") }
    println(log)
  }
}
