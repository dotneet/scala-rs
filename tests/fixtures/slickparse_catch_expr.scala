// `try b catch h` where `h` is a PartialFunction value rather than a block of
// case clauses (nsc `makeCatchFromExpr`). The handler runs only when the body
// throws, and an exception it does not accept is rethrown unchanged.
object Main {
  val ignore: PartialFunction[Throwable, Unit] = { case _: RuntimeException => println("ignored") }

  var made = 0
  def mk(): PartialFunction[Throwable, Unit] = {
    made += 1
    println("mk")
    ignore
  }

  val toLen: PartialFunction[Throwable, Int] = { case e: IllegalStateException => e.getMessage.length }

  def main(args: Array[String]): Unit = {
    // No exception: the handler is never evaluated.
    try println("body ok") catch mk()
    println("made=" + made)

    // Caught: handler evaluated once, then applied.
    try throw new RuntimeException("boom") catch mk()
    println("made=" + made)

    // Not accepted by the handler: rethrown unchanged.
    try {
      try throw new Exception("passed through") catch mk()
    } catch {
      case e: Exception => println("outer saw " + e.getMessage)
    }
    println("made=" + made)

    // A `try`/`catch` in value position, and a `finally` after the handler.
    val n = try throw new IllegalStateException("abcd") catch toLen
    println("n=" + n)

    try {
      throw new RuntimeException("with finally")
    } catch mk() finally println("finally ran")
    println("made=" + made)

    // A braced expression that is not case clauses is still a value.
    try throw new RuntimeException("braced") catch { ignore }
    println("done")
  }
}
