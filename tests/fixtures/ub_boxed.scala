// `()` boxes to the `BoxedUnit` singleton, never to `null`: that is why
// `println(x: Any)` prints `()`, why `(x: Any) == ()` is true, and why a
// `case () =>` pattern must *not* also catch `null`.
object Main {
  def id[A](a: A): A = a
  def show(a: Any): String = String.valueOf(a)

  def classify(a: Any): String = a match {
    case ()   => "unit"
    case null => "null"
    case _    => "other"
  }

  def main(args: Array[String]): Unit = {
    println(id(()))
    println(show(()))
    println(((): Any) == ())
    println(((): Any) == null)
    println(classify(()))
    println(classify(null))
    println(classify(1))
    val any: Any = ()
    println(any)
    println(any.toString)
    println(any.hashCode)
    // Discarded, the reference `id` returned has to be popped, or the loop's
    // back edge merges two different stack heights (nsc pops it too).
    var i = 0
    while (i < 2) {
      id(())
      i += 1
    }
    if (args.length == 0) id(()) else id(())
    println(i)
    // `asInstanceOf[Unit]` is a `Unit` expression: nsc drops the receiver and
    // materialises `UNIT` where the result is used. `isInstanceOf[Unit]` is
    // `instanceof scala/runtime/BoxedUnit`.
    println(any.asInstanceOf[Unit])
    var j = 0
    while (j < 2) {
      any.asInstanceOf[Unit]
      j += 1
    }
    println(j)
    println(any.isInstanceOf[Unit])
    println((1: Any).isInstanceOf[Unit])
  }
}
