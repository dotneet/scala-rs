// nsc `expr(InBlock)`: a function literal written as a *block statement*
// takes the rest of the block as its body, so `{ x => val n = 1; n }` is a
// lambda over a block, not `val` in expression position. And nsc's
// `typeOrInfixType`: an ascription in block position stops at `InfixType`, so
// the `=>` of `{ x: Int => body }` belongs to the lambda, not to a function
// type. Only Function0/Function1 are used so the private runtime suffices.
// (Named with the `xsource3` prefix only to keep fixture names unique.)
object Main {
  def apply1(f: Int => Int, x: Int): Int = f(x)

  def thunk(f: () => Int): Int = f()

  def main(args: Array[String]): Unit = {
    // one-liner with `;` separators
    println(apply1({ x => val n = x + 1; n * 2 }, 3))

    // multi-line block body
    println(apply1(
      { x =>
        val n = x + 1
        val m = n * 3
        m
      },
      3
    ))

    // a `def` inside the body
    println(apply1(
      { x =>
        def twice: Int = x * 2
        val y = twice + 1
        y
      },
      5
    ))

    // typed parameter without parentheses, block body
    val g = { x: Int =>
      var acc = 0
      acc = acc + x
      acc
    }
    println(g(9))

    // zero-parameter literal in block position
    val t = { () =>
      val a = 3
      a * 7
    }
    println(thunk(t))

    // lambda inside a `case` body, which is itself a block
    val h = (i: Int) =>
      i match {
        case 0 =>
          val z = 100
          z
        case k =>
          val f2 = { y: Int =>
            val u = y * k
            u
          }
          f2(2)
      }
    println(h(0))
    println(h(4))

    // nested lambda in block position
    println(apply1({ x =>
      val inner = { y: Int =>
        val q = y + x
        q
      }
      inner(10)
    }, 1))
  }
}
