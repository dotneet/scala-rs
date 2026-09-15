trait NestedResultRep[A]

object NestedLambdaResult {
  def some[A](a: A): NestedResultRep[Option[A]] = null

  final class Box[A] {
    def flatMap[B](f: A => NestedResultRep[Option[B]]): NestedResultRep[Option[B]] = null
  }

  val inferred = new Box[Int].flatMap(i => some(i.toString))
  val ordinary = new Box[Int].flatMap(i => some(i + 1))
}
