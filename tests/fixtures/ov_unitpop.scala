// Discarding the result of a member whose *declared* result type is a type
// parameter instantiated at `Unit`.
//
// `trait Box[A] { def get: A }` compiles to `get()Ljava/lang/Object;`, so
// `b.get` on a `Box[Unit]` leaves a reference on the stack even though the
// expression's Scala type is `Unit`. A **nilary** `def` has no argument list,
// so the call is a bare `Select` with no `Apply` above it, and the statement
// discard never popped it. Straight-line code got away with that; the first
// branch afterwards -- the `try` here -- needs a stackmap frame and turns it
// into `VerifyError: Inconsistent stackmap frames`.
//
// `next()` (an *empty-paren* def, so an `Apply`) was already handled; it is
// kept here so the two shapes stay together.
//
// Only string operations, so this runs under the private runtime as well as
// against the real scala-library jar.
object Main {
  trait It[A] { def next(): A }
  trait Box[A] { def get: A }

  def viaApply(): Unit = {
    val i = new It[Unit] { def next(): Unit = println("a") }
    i.next()
    try { println("t1") } catch { case _: Throwable => println("c1") }
  }

  def viaNilarySelect(): Unit = {
    val b: Box[Unit] = new Box[Unit] { def get: Unit = println("b") }
    b.get
    try { println("t2") } catch { case _: Throwable => println("c2") }
  }

  // The same call where `A` is still abstract at the call site.
  def viaTypeParam[A](i: It[A]): Unit = {
    i.next()
    try { println("t3") } catch { case _: Throwable => println("c3") }
  }

  // A `while` needs a frame at its back edge, so it is the other shape that
  // used to expose the leftover reference.
  def viaLoop(b: Box[Unit]): Unit = {
    var n = 0
    while (n < 2) {
      b.get
      n += 1
    }
    println("t4")
  }

  def main(args: Array[String]): Unit = {
    viaApply()
    viaNilarySelect()
    viaTypeParam(new It[Unit] { def next(): Unit = println("p") })
    viaLoop(new Box[Unit] { def get: Unit = println("l") })
  }
}
