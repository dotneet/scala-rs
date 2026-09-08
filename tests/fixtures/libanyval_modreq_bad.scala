// A repeated parameter against a repeated parameter is *not* a shape
// difference: `A*` read at `IntSink` is `Int*`, exactly what `take` declares,
// so this overrides a concrete member and needs the `override` modifier.
//
// Pins the recursion in the repeated-parameter rule. Treating `Repeated`
// against anything as "certainly different" without first checking that the
// other side is also repeated would silently accept this.
trait Sink[A] {
  def take(xs: A*): Int = 1
}
class IntSink extends Sink[Int] {
  def take(xs: Int*): Int = 0
}
object Main {
  def main(args: Array[String]): Unit = println(new IntSink().take(1, 2))
}
