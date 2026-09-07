// Overload specificity, SLS 6.26.3, for the `PartialFunction.andThen` pair:
//
//   def andThen[C](k: B => C): PartialFunction[A, C]
//   def andThen[C](k: PartialFunction[B, C]): PartialFunction[A, C]
//
// Both are applicable to a function literal, so which one runs is decided by
// specificity -- and the two behave *differently at run time*: the second
// composes the two domains, the first keeps only the receiver's. Every
// `isDefinedAt` below is therefore a check on the alternative that was picked,
// not just on the program compiling. The expected output is real scalac
// 2.13.16's.
package ovspec

object Main {
  val pf: PartialFunction[Int, String] = { case 1 => "one"; case 2 => "two" }
  val kf: PartialFunction[String, Int] = { case "one" => 1 }

  // nsc's shape for an un-annotated literal is `Function1[Any, Nothing]`,
  // which is not a `PartialFunction` and is not SAM-convertible to one, so
  // `preSelectOverloaded` leaves only `k: B => C`. The receiver's domain
  // survives untouched.
  val a: PartialFunction[Int, Int] = pf.andThen(s => s.length)

  // An annotated literal has the same shape and picks the same alternative.
  val b: PartialFunction[Int, Int] = pf.andThen((s: String) => s.length)

  // A `{ case … }` literal's shape is `PartialFunction[Any, Nothing]`: both
  // alternatives are applicable and the `PartialFunction` one is strictly the
  // more specific, so the domains compose and `2` drops out.
  val c: PartialFunction[Int, Int] = pf.andThen { case "one" => 1 }

  // The same choice with no literal anywhere -- pure specificity.
  val d: PartialFunction[Int, Int] = pf.andThen(kf)

  // `compose` is inherited from `Function1` alone; `PartialFunction` declares
  // no second alternative, so nothing is weighed.
  val e: Int => String = pf.compose((i: Int) => i + 1)

  // The same pair reached through `Function1`'s own `andThen` on a plain
  // function value: only one alternative exists there.
  val g: Int => Int = ((i: Int) => i * 2).andThen(i => i + 1)

  // A hand-written pair with the same disagreement, where the *second*
  // parameter is what distinguishes the alternatives. A plain literal's shape
  // rules out the `PartialFunction` alternative before the second parameter is
  // ever weighed; a `{ case … }` literal leaves both, and `String` beats `Any`.
  def n(f: Int => Int, s: String): Int = 1
  def n(f: PartialFunction[Int, Int], s: Any): Int = 2
  def p(f: PartialFunction[Int, Int], s: String): Int = 3
  def p(f: PartialFunction[Int, Int], s: Any): Int = 4

  def main(args: Array[String]): Unit = {
    println(a(1))
    println(a.isDefinedAt(2))
    println(b(2))
    println(b.isDefinedAt(2))
    println(c(1))
    println(c.isDefinedAt(2))
    println(d(1))
    println(d.isDefinedAt(2))
    println(e(0))
    println(g(3))
    println(n(x => x, "a"))
    println(p({ case 1 => 1 }, "a"))
  }
}
