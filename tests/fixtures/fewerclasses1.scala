// Cases that used to be emitted more than once. Each shape is here because it
// multiplied classfiles, and two of them are also wrong at run time.
//
//  1. A `{ case … }` literal nested inside another one's case body. The
//     closure class generated its case bodies into `apply` and again into
//     `applyOrElse`, so the inner literal came out twice (and 2^depth times
//     when nested further). `apply` now delegates to `applyOrElse`, which is
//     what `AbstractPartialFunction` does; a `null` fallback is the
//     `MatchError` that `apply` owes.
//  2. A call that omits default arguments. The `name$default$n` getters took
//     the whole preceding parameter prefix, so the argument trees went into
//     the call, into getter 2 and into getter 3 -- 2^k copies for k omitted
//     defaults.
//  3. The receiver of such a call was spliced into every getter call, so it
//     was **evaluated** once per omitted default. `mk().infer()` ran `mk()`
//     three times; real scalac runs it once.

class Ops(val n: Int) {
  def replace(f: PartialFunction[Int, Int], keepType: Boolean = false, bottomUp: Boolean = false): Int =
    f.applyOrElse(n, (x: Int) => x)
  def infer(scope: Int = 0, deep: Boolean = false): Int = n + scope
}

// nsc rejects a default that names an earlier parameter of its own clause;
// scala-rs accepts it, so the getter still has to take that parameter.
object SameClause {
  def f(x: Int, y: Int = 5, z: Int = 9): Int = x + y + z
}

object Counter {
  var receivers = 0
}

object Main {
  val outer: PartialFunction[Any, String] = {
    case i: Int =>
      val inner: PartialFunction[Any, String] = { case s: String => "in(" + s + ")" }
      inner.applyOrElse("i" + i, (a: Any) => "no")
    case s: String => "out(" + s + ")"
  }

  def mk(): Ops = { Counter.receivers += 1; new Ops(7) }

  def main(args: Array[String]): Unit = {
    println(outer(1))
    println(outer("a"))
    println(outer.isDefinedAt(1.0))
    println(outer.applyOrElse(2.0, (x: Any) => "fallback"))
    // No case matches and there is no fallback: `MatchError`, from `apply`.
    try println(outer(2.0))
    catch { case _: Throwable => println("threw") }

    println(mk().infer())
    println("receivers=" + Counter.receivers)
    println(mk().infer(2))
    println("receivers=" + Counter.receivers)

    val o = new Ops(3)
    println(o.replace({ case x if x > 0 => x + 1 }))
    println(o.replace({ case x if x > 0 => x + 2 }, bottomUp = true))
    println(o.replace({ case x if x > 0 => x + 3 }, keepType = true, bottomUp = true))

    println(SameClause.f(1))
    println(SameClause.f(1, 2))
    println(SameClause.f(1, 2, 3))
  }
}
