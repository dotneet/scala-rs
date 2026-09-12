// Chain / NonEmptyChain / Ior / State / Eval: the data types whose
// implementations lean hardest on pattern matching over sealed hierarchies and
// on tail-recursive loops.
import cats._
import cats.data._
import cats.syntax.all._

object Main {
  def main(args: Array[String]): Unit = {
    val c = Chain(1, 2, 3)
    println(c)
    println(c.toList)
    println(c.map(_ + 1).toList)
    println(c.filter(_ % 2 == 1).toList)
    println((c ++ Chain(4)).toList)
    println(c.reverse.toList)
    println(c.length)
    println(Chain.empty[Int].isEmpty)
    println(c.headOption)
    println(c.lastOption)
    println(Monoid[Chain[Int]].combine(c, Chain(9)).toList)
    println(Foldable[Chain].foldMap(c)(_.toString))
    println(Traverse[Chain].traverse(c)(i => Option(i * 2)).map(_.toList))
    println(c.uncons.map { case (h, t) => (h, t.toList) })

    val nec = NonEmptyChain(1, 2, 3)
    println(nec)
    println(nec.head)
    println(nec.toChain.toList)
    println(nec.map(_ * 3).toChain.toList)
    println(nec.reduceLeft(_ + _))
    println(NonEmptyChain.fromChain(Chain(1)))
    println(NonEmptyChain.fromChain(Chain.empty[Int]))
    println(NonEmptyChain.one(4))

    // Ior accumulates on the left and keeps the right
    val both: Ior[String, Int] = Ior.both("w", 1)
    println(both)
    println(both.left)
    println(both.right)
    println(both.map(_ + 1))
    println(Ior.left[String, Int]("l"))
    println(Ior.right[String, Int](2))
    println(Ior.both(NonEmptyChain("a"), 1).isBoth)
    println(both.flatMap(i => Ior.both("x", i * 2)))
    println(Semigroupal[Ior[String, *]].product(both, Ior.right[String, Int](5)))

    // State
    val st: State[Int, String] = State(s => (s + 1, "s=" + s))
    println(st.run(1).value)
    println(st.runA(2).value)
    println(st.runS(3).value)
    println(st.flatMap(a => State[Int, String](s => (s * 2, a + "/" + s))).run(1).value)
    println(State.get[Int].run(5).value)
    println(State.set[Int](9).run(0).value)
    println(State.modify[Int](_ + 10).run(1).value)
    println(State.pure[Int, String]("p").run(0).value)
    println((1 to 4).toList.traverse(i => State[Int, Int](s => (s + i, s))).run(0).value)

    // Eval
    println(Eval.now(1).value)
    println(Eval.later(2).value)
    println(Eval.always(3).value)
    println(Eval.now(1).map(_ + 1).value)
    println(Eval.now(1).flatMap(i => Eval.later(i + 5)).value)
    println(Monad[Eval].pure(7).value)

    // a deep, stack-safe fold, to force the trampolines
    println(Foldable[List].foldRight((1 to 20000).toList, Eval.now(0))((a, b) => b.map(_ + a)).value)
    def loop(i: Int): Eval[Int] =
      if (i == 0) Eval.now(0) else Eval.defer(loop(i - 1)).map(_ + 1)
    println(loop(20000).value)
  }
}
