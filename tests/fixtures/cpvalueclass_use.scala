// The client. Compiled by real scalac and by scala-rs against the same
// scalac-built `cpvalueclass_lib`; the two must print the same bytes.
//
// One shape remains deliberately absent because it fails for a reason that
// has nothing to do with reading a value class from `-cp`:
//
//   * `def box(m: Meters): Any` -- a `-cp` method whose *parameter* is a value
//     class is installed at its erased descriptor, so the call is declined
//     with "no matching overload for (Int)AnyRef with arguments (Meters)".
import cpvc._

object Main {
  def main(args: Array[String]): Unit = {
    val m = Factory.make(3)
    println(m.describe)
    println(m.plus(4).describe)
    println(m.raw)
    val n = Factory.named("hi")
    println(n.shout)
    val direct = new Meters(9)
    println(direct.describe)
    println(direct.n)
    // The fs2 shape: `Holder.partial(true)(it, chunkSize = 1)`, i.e. an
    // `$extension` on a *nested* value class's companion module, with
    // arguments after the receiver.
    println(Holder.partial(true)(Iterator(1, 2, 3), 1))
    println(Holder.partial(false).apply(Iterator("a"), 2))
    // A universal trait on a value class.
    val t = Factory.tagged("x")
    println(t.label)
    println((t: Described).label)
    // A value class in a reference position: boxed into the list, unboxed out.
    println(Factory.firstOf(List(new Meters(1), new Meters(2))).describe)
    println(List(new Meters(5)).map(_.raw))
  }
}
