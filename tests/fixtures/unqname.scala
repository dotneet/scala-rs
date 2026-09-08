// `agent/unqualname`: two unrelated roots behind `not found: value` in the
// standard library, both about which symbol a simple name denotes.
//
// (a) `java.lang.Object`'s monitor methods were not declared on `AnyRef` at
//     all, so `wait()` / `notify()` / `notifyAll()` were unresolved in every
//     position -- unqualified, through `this`, and on another `AnyRef`.
//     `scala/concurrent/{Channel,SyncVar}.scala` are the library's users.
//
// (b) A builtin type name was mapped to its primitive `Type` by the name
//     alone, ahead of any scope lookup, so a `trait Int` in an enclosing
//     template lost to `scala.Int` (SLS 2 puts a definition at level 1 and
//     the `scala._` wildcard at level 3). The standard library's
//     `scala.collection.generic.BitOperations` is exactly this shape --
//     `object Int extends Int` -- and it compiled *silently* to
//     `extends java.lang.Integer`, which is why `TreeSeqMap` could not see
//     `zero` / `mask` / `hasMatch` / `highestOneBit`.

// --- (a) -------------------------------------------------------------------

class Cell {
  private var full = false
  private var value = 0
  // Unqualified, inside the class body: the shape `SyncVar.put` writes.
  def put(v: Int): Unit = synchronized { value = v; full = true; notifyAll() }
  // `while (!isDefined) wait()`, the shape `SyncVar.get` writes.
  def take(): Int = synchronized { while (!full) wait(); full = false; value }
  def poke(): Unit = synchronized { notify() }
  // Through `this`, and the two timed `wait` overloads. Both return on their
  // own timeout, so no second thread is needed to make them terminate.
  def qualified(): Unit = synchronized { this.notifyAll() }
  def timed(): Boolean = synchronized { wait(1L); true }
  def timedNanos(): Boolean = synchronized { wait(1L, 0); true }
}

// --- (b) -------------------------------------------------------------------

// `BitOperations`'s own shape, down to the `type Int = scala.Int` member.
object Bits {
  trait Int {
    type Int = scala.Int
    def zero(i: Int, mask: Int): Boolean = (i & mask) == 0
    def complement(i: Int): Int = (-1) ^ i
    def mask(i: Int, m: Int): Int = i & (complement(m - 1) ^ m)
  }
  // Parents position. This is the one that used to compile to
  // `extends java.lang.Integer` with no diagnostic at all.
  object Int extends Int
  class Uses extends Int
  // Type-annotation position, in the same template.
  def viaType(x: Int): Boolean = x.zero(4, 3)
}

// An *explicit* import is SLS level 2 and also outranks the `scala._`
// wildcard, so `Int` here is `Bits.Int` too.
object Imp {
  import Bits.Int
  def viaImport(x: Int): Boolean = x.zero(8, 7)
}

object Main {
  def main(args: Array[String]): Unit = {
    val c = new Cell
    val t = new Thread(new Runnable { def run(): Unit = c.put(42) })
    t.start()
    println(c.take())
    t.join()
    c.poke()
    c.qualified()
    println(c.timed())
    println(c.timedNanos())
    val o: AnyRef = new Object
    o.synchronized { o.notifyAll() }

    println(Bits.Int.zero(4, 3))
    println(Bits.viaType(Bits.Int))
    println(new Bits.Uses().mask(29, 4))
    println(Imp.viaImport(Bits.Int))
  }
}
