object Main {
  // The `throw` is the whole body: no `ireturn` may follow the `athrow`.
  def boom(): Int = throw new RuntimeException("boom")

  // One arm of the `if` never returns; the merge point must still see the
  // other arm's value.
  def half(c: Boolean): Int = if (c) 7 else throw new RuntimeException("half")

  // Both arms throw: the method has no reachable return at all.
  def both(c: Boolean): Int =
    if (c) throw new RuntimeException("t") else throw new IllegalStateException("f")

  // A `match` whose last case throws.
  def pick(n: Int): String = n match {
    case 0 => "zero"
    case 1 => "one"
    case _ => throw new IllegalArgumentException("pick " + n)
  }

  // Non-local return out of a closure, with unreachable code behind it.
  def firstPositive(xs: List[Int]): Int = {
    xs.foreach((x: Int) => if (x > 0) return x)
    0
  }

  // `return` in the middle of a block: the trailing statements are dead.
  def early(n: Int): Int = {
    if (n > 0) {
      return n * 2
    }
    val fallback = -1
    fallback
  }

  // `return` out of a guarded body still has to run the finalizer, so the
  // emitter may not simply drop everything behind the `return`.
  def earlyFin(n: Int): Int = {
    try {
      if (n > 0) return n * 10
      1
    } finally {
      println("fin3")
    }
  }

  // `return` out of a `synchronized` block must release the monitor.
  def earlySync(n: Int): Int = Main.synchronized {
    if (n > 0) return n + 100
    2
  }

  // The try body always throws, so the normal-completion copy of `finally` is
  // unreachable; the handler copy still has to run.
  def alwaysThrows(): String = {
    try {
      throw new RuntimeException("inner")
    } finally {
      println("fin")
    }
  }

  // A catch clause that itself throws, under a `finally`.
  def catchThrows(): String = {
    try {
      try {
        throw new RuntimeException("a")
      } catch {
        case _: RuntimeException => throw new IllegalStateException("b")
      } finally {
        println("fin2")
      }
    } catch {
      case e: IllegalStateException => "caught " + e.getMessage
    }
  }

  def msg(f: () => Int): String =
    try { "v" + f() }
    catch { case e: RuntimeException => "e" + e.getMessage }

  def main(args: Array[String]): Unit = {
    println(msg(() => boom()))
    println(half(true))
    println(msg(() => half(false)))
    println(msg(() => both(true)))
    println(pick(0))
    println(pick(1))
    try println(pick(2))
    catch { case e: IllegalArgumentException => println("bad " + e.getMessage) }
    println(firstPositive(1 :: 2 :: Nil))
    println(firstPositive((-1) :: 3 :: Nil))
    println(firstPositive((-1) :: (-2) :: Nil))
    println(early(3))
    println(early(-3))
    println(earlyFin(4))
    println(earlyFin(-4))
    println(earlySync(5))
    println(earlySync(-5))
    try println(alwaysThrows())
    catch { case e: RuntimeException => println("outer " + e.getMessage) }
    println(catchThrows())
  }
}
