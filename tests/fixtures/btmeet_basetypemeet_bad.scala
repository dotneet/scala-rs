// The restrictions on the base-type meet. Every one of these is rejected by
// real scalac 2.13.16 at the same line, which
// `crates/cli/tests/btmeet.rs::scalac_rejects_btmeet_bad_at_the_same_lines`
// asserts against scalac directly.
//
// The rule is nsc's `mergePrefixAndArgs(variants, Variance.Contravariant, _)`:
// a covariant parameter takes the glb of the arrivals, a contravariant one the
// lub. It is not "take the most derived class in sight", it is not "take the
// most derived arrival regardless of variance", and it does not make an
// invariant clash legal.

trait IterOps[+A, +C] {
  def me: C
  def tail: C = me
}
trait Iter[+A] extends IterOps[A, Iter[A]]
trait LinOps[+A, +C] extends IterOps[A, C]

class Str[+A](val a: A) extends LinOps[A, Str[A]] with Iter[A] {
  def me: Str[A] = this
  // The glb of `Iter[A]` and `Str[A]` is `Str[A]`. `Sub` is below `Str` and is
  // not in either arrival, so the meet does not reach it.
  def bad1: String = tail.onlyOnSub
}

class Sub[+A](b: A) extends Str[A](b) {
  def onlyOnSub: String = "sub"
}

class Animal
class Dog extends Animal

trait Sink[-T] {
  def take(t: T): String = "sink"
}
trait Wide[-T] extends Sink[T]
trait Narrow extends Sink[Dog]

final class Both extends Wide[Animal] with Narrow

object Bad2 {
  // `Sink`'s parameter is contravariant, so the merge is the lub of `Animal`
  // and `Dog` -- `Animal`, and nothing above it.
  def bad2: String = new Both().take("not an animal")
}

object Bad3 {
  // The merged `C` is `Str[Int]`, which is a restriction as much as a repair:
  // it is not `Animal`, and it is not widened to anything else either. Before
  // the merge this line read `found: Iter[Int]`, which is the wrong answer to
  // the same question.
  def bad3(s: Str[Int]): Animal = s.tail
}
