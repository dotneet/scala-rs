// The `@tailrec` shapes nsc's `TailCalls` still rejects, so that widening the
// eligibility rule for `tt_tailrec.scala` is pinned to stop where nsc stops.
// scalac 2.13.16 reports "could not optimize @tailrec annotated method … it is
// neither private nor final so can be overridden" on every one of the five.

import scala.annotation.tailrec

// A public member of an ordinary class.
class Bad1 {
  @tailrec def go(n: Int): Int = if (n <= 0) 0 else go(n - 1)
}

// A trait's member.
trait Bad2 {
  @tailrec def go(n: Int): Int = if (n <= 0) 0 else go(n - 1)
}

// A `sealed` class whose subclass *does* override it.
sealed class Bad3 {
  @tailrec def go(n: Int): Int = if (n <= 0) 0 else go(n - 1)
}
final class Bad3Sub extends Bad3 {
  override def go(n: Int): Int = n
}

// A class that is a member of an object can be extended from anywhere.
object Bad4 {
  class K {
    @tailrec def go(n: Int): Int = if (n <= 0) 0 else go(n - 1)
  }
}

// A block-local class that another class in the same block overrides.
object Bad5 {
  def mk(): Int = {
    class L {
      @tailrec def go(n: Int): Int = if (n <= 0) 0 else go(n - 1)
    }
    class LSub extends L {
      override def go(n: Int): Int = n
    }
    new LSub().go(1)
  }
}
