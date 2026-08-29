// `FunctionN.tupled` / `curried` and `scala.Function.untupled`, the members
// slick's `lifted/CompilableFunctions.scala` builds every `CompiledFunction`
// arity out of. They are `scala-library` members (interface default methods on
// `scala/FunctionN`, and `scala/Function$` for `untupled`), so this fixture is
// library-ABI only; `ctacc_fn_bad.scala` pins the diagnostic the private
// runtime gives instead.

object Main {
  // A parameterless method whose result is a function: `f.tupled(t)` is
  // `f.tupled.apply(t)`, and so is this.
  def adder: (Int, Int) => Int = (a, b) => a + b

  def main(args: Array[String]): Unit = {
    val f2: (Int, Int) => Int = (a, b) => a + b
    val t2 = f2.tupled
    println(t2((3, 4)))
    println(f2.tupled((5, 6)))
    println(f2.curried(3)(4))

    val u2 = Function.untupled(t2)
    println(u2(10, 20))

    val f3: (Int, String, Long) => String = (a, b, c) => a.toString + b + c.toString
    println(f3.tupled((1, "x", 2L)))
    println(f3.curried(1)("y")(3L))
    println(Function.untupled(f3.tupled)(4, "z", 5L))

    val f5: (Int, Int, Int, Int, Int) => Int = (a, b, c, d, e) => a + b + c + d + e
    println(f5.tupled((1, 2, 3, 4, 5)))
    println(f5.curried(1)(2)(3)(4)(5))
    println(Function.untupled(f5.tupled)(2, 3, 4, 5, 6))

    // Arity 22, the highest scala-library defines.
    val f22: (Int, Int, Int, Int, Int, Int, Int, Int, Int, Int, Int, Int, Int, Int, Int, Int, Int,
      Int, Int, Int, Int, Int) => Int =
      (a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14, a15, a16, a17, a18, a19, a20,
       a21, a22) =>
        a1 + a2 + a3 + a4 + a5 + a6 + a7 + a8 + a9 + a10 + a11 + a12 + a13 + a14 + a15 + a16 +
          a17 + a18 + a19 + a20 + a21 + a22
    println(f22.tupled((1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1)))

    println(adder(7, 8))
  }
}
