// `@tailrec` on a method that nsc's `TailCalls` considers ineligible for
// override, and that this compiler rejected. nsc's predicate is
// `Symbol.isEffectivelyFinalOrNotOverridden`, so besides `final` / `private` /
// a member of an `object` it also accepts:
//
// 1. a member of the `$anon` class of a `new C { … }` -- nsc gives that class
//    the FINAL flag. cats writes 7 of these;
// 2. a member of a `sealed` class that no subclass overrides;
// 3. a member of a class declared inside a block, which can only be extended
//    from inside that block;
// 4. a `def` in a `val`'s right-hand side, which is not a member of anything.
//    Its symbol is nevertheless *owned* by the enclosing class -- there is no
//    accessor symbol to own it -- so the owner cannot be used to tell it from
//    a real member. cats writes 10 of these, one per
//    `instances/{eq,order,show,…}.scala`.
//
// `tt_tailrec_bad.scala` holds the shapes nsc still rejects. scalac 2.13.16
// accepts this file as it stands.

import scala.annotation.tailrec

trait Loop { def go(n: Int, acc: Int): Int }

// 2
sealed class Sealed {
  @tailrec final private def unused(n: Int): Int = if (n <= 0) 0 else unused(n - 1)
  @tailrec def go(n: Int, acc: Int): Int = if (n <= 0) acc else go(n - 1, acc + n)
  def unusedRef: Int = unused(0)
}
// A subclass that does not override `go` leaves it un-overridden.
final class SealedSub extends Sealed

class Outer {
  // 4
  class Inner(n0: Int) {
    lazy val r: Int = {
      @tailrec def loop(k: Int, acc: Int): Int = if (k <= 0) acc else loop(k - 1, acc + k)
      loop(n0, 0)
    }
  }
  def make(n: Int): Int = new Inner(n).r
}

object Main {
  // 1
  val anon: Loop = new Loop {
    @tailrec def go(n: Int, acc: Int): Int = if (n <= 0) acc else go(n - 1, acc + n)
  }

  // 3
  def local(n: Int): Int = {
    class L {
      @tailrec def go(k: Int, acc: Int): Int = if (k <= 0) acc else go(k - 1, acc + k)
    }
    new L().go(n, 0)
  }

  def main(args: Array[String]): Unit = {
    println(anon.go(4, 0))
    println(new Sealed().go(4, 0))
    println(new SealedSub().go(5, 0))
    println(local(4))
    println(new Outer().make(4))
  }
}
