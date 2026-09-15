trait NestedResultBadRep[A]

object NestedLambdaResultBad {
  def some[A](a: A): NestedResultBadRep[Option[A]] = null

  final class Box[A] {
    def flatMap[B](f: A => NestedResultBadRep[Option[B]]): NestedResultBadRep[Option[B]] = null
  }

  val wrong: NestedResultBadRep[Option[Int]] =
    new Box[Int].flatMap(i => some(i.toString))
}
