// nsc's `isInProperSubClassOf`, the tie-break `Infer.isStrictlyMoreSpecific`
// applies when neither alternative's signature is more specific than the
// other's: the one whose *owner* is the proper subclass wins.
//
// `class AndThen[-T, +R] extends (T => R)` (cats `data/AndThen.scala`) writes
// its parent as a function type, and the overriding `andThen` / `compose` it
// declares differ from `Function1`'s only in the result type -- so each is as
// specific as the other and the owner relation is the only thing that can
// separate them. Which one runs is observable: `AndThen`'s remember their
// composition, `Function1`'s do not.
//
// The expected output is real scalac 2.13.16's.
package ovspec

final class AndThen[-T, +R](val trace: String, val f: T => R) extends (T => R) {
  def apply(t: T): R = f(t)
  override def andThen[A](g: R => A): AndThen[T, A] =
    new AndThen(trace + ">a", (t: T) => g(f(t)))
  override def compose[A](g: A => T): AndThen[A, R] =
    new AndThen(trace + ">c", (a: A) => f(g(a)))
}

// The same shape with the parent spelled as the class rather than as `=>`.
final class Chain[T, R](val trace: String, val f: T => R) extends Function1[T, R] {
  def apply(t: T): R = f(t)
  override def andThen[A](g: R => A): Chain[T, A] = new Chain(trace + "+a", (t: T) => g(f(t)))
}

object Main {
  val at = new AndThen[Int, Int]("at", i => i + 1)
  val ch = new Chain[Int, Int]("ch", i => i * 2)

  // Both `AndThen.andThen` and the inherited `Function1.andThen` are
  // applicable and equally specific; the subclass's owner decides.
  val a = at.andThen((i: Int) => i * 10)
  val c = at.compose((i: Int) => i - 1)
  val d = ch.andThen((i: Int) => i - 3)

  // An un-annotated literal takes the same route.
  val e = at.andThen(i => i * 100)

  def main(args: Array[String]): Unit = {
    println(a.trace)
    println(a(1))
    println(c.trace)
    println(c(1))
    println(d.trace)
    println(d(4))
    println(e.trace)
    println(e(1))
  }
}
