// `new C(args)` on a *secondary* constructor.
//
// The typer picks the constructor alternative and the backend emits the
// `invokespecial` at that alternative's descriptor. Erasure has to adapt the
// arguments against the same one; it used to adapt them against the class's
// first `<init>` member -- the primary -- so `new Sec("abcd")` unboxed a
// `String` to `int` and boxed it back while the call named
// `(Ljava/lang/String;)V`. That compiles and is a `VerifyError` at run time,
// which is why every case here is printed rather than merely compiled.

class Sec(val a: Int) {
  def this(s: String) = this(s.length)
  // from inside the class
  def viaSecondary: Sec = new Sec("abcd")
}

object Sec {
  // from the companion
  def fromCompanion: Sec = new Sec("xyz")
}

// Two secondaries whose erased descriptors differ in exactly one parameter.
class Pair(val k: String, val v: Int) {
  def this(k: String, v: String) = this(k, v.length)
  def this(k: Int, v: Int) = this("i" + k.toString, v)
}

// A secondary that delegates to another secondary rather than to the primary.
class Chain(val d: Int) {
  def this(s: String) = this(s.length)
  def this(b: Boolean) = this(if (b) "yes" else "no")
}

// A value class in a secondary constructor's parameter list. The primary
// takes a reference and the secondary a value class that erases to `int`, so
// adapting against the wrong one is a `checkcast`/unbox in either direction.
class Meters(val n: Int) extends AnyVal
class Dist(val m: String) {
  def this(x: Meters) = this("m" + (x.n * 2).toString)
}

// A default argument, in a class that also has secondary constructors: the
// `new` that takes the default must still name the primary's descriptor while
// the secondaries are candidates, and a secondary may itself delegate to the
// primary through the default.
//
// The default is on the *primary* because a default on a secondary
// constructor is a separate, pre-existing gap:
// `synthesize_ctor_default_getters` only ever runs over the primary's
// parameters, so `def this(p: String, q: String = "dq")` is rejected with
// `value <init>$default$2 is not a member of Main$`. That is a hard error on
// an unmodified build of the branch point too, not this slice's silent
// miscompile.
class Deft(val p: Int, val q: String = "dq") {
  def this(s: String) = this(s.length, "s" + s)
  def this(b: Boolean) = this(if (b) 1 else 0)
}

object Main {
  def main(args: Array[String]): Unit = {
    // the primary path, pinned in the same program
    println("Sec/primary " + new Sec(7).a.toString)
    println("Sec/inside " + new Sec(1).viaSecondary.a.toString)
    println("Sec/companion " + Sec.fromCompanion.a.toString)
    println("Sec/outside " + new Sec("hello").a.toString)

    val p1 = new Pair("a", 3)
    println("Pair/primary " + p1.k + " " + p1.v.toString)
    val p2 = new Pair("b", "cdef")
    println("Pair/ss " + p2.k + " " + p2.v.toString)
    val p3 = new Pair(5, 9)
    println("Pair/ii " + p3.k + " " + p3.v.toString)

    println("Chain/primary " + new Chain(4).d.toString)
    println("Chain/string " + new Chain("abcde").d.toString)
    println("Chain/true " + new Chain(true).d.toString)
    println("Chain/false " + new Chain(false).d.toString)

    println("Dist/primary " + new Dist("plain").m)
    println("Dist/vc " + new Dist(new Meters(6)).m)

    val d1 = new Deft(2, "q")
    println("Deft/primary " + d1.p.toString + " " + d1.q)
    val d2 = new Deft(5)
    println("Deft/default " + d2.p.toString + " " + d2.q)
    val d3 = new Deft("abc")
    println("Deft/secondary " + d3.p.toString + " " + d3.q)
    val d4 = new Deft(true)
    println("Deft/viadefault " + d4.p.toString + " " + d4.q)
  }
}
