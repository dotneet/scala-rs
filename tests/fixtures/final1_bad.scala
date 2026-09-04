// The rejection side of agent/final1. Pins that what we relaxed did not tip over
// into quietly accepting. Real scalac 2.13.16 rejects these two as well.
object Main {
  // We now widen the self alias to look for `apply`, but a class that has no
  // `apply` must still be told "is not a member" as before.
  final class NoApply(val n: Int) { self =>
    def get(i: Int): Int = self(i)
  }

  // The expected type is stronger than the argument in an invariant position,
  // but only when the argument's solution conforms to it. If it does not, the
  // type mismatch stands.
  def wrong: Set[Int] = Set() ++ Some("x")

  // `Option.option2Iterable` is `Option[A] => Iterable[A]`. Because a wildcard
  // unifies with anything, this counted as "the shapes matched" when there was
  // nothing to solve at all. Real scalac rejects it too.
  trait ColOpt[+T]
  final case class DefaultOpt[T](v: T) extends ColOpt[T]
  def notAView(d: Option[DefaultOpt[_]]): IterableOnce[ColOpt[Nothing]] = d

  def main(args: Array[String]): Unit = {
    println(new NoApply(1).get(0))
    println(wrong)
    println(notAView(None))
  }
}
