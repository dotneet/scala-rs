trait Join[-L, -R, +Out]

object Join {
  implicit def widen[A]: Join[A, A, A] = new Join[A, A, A] {}
}

trait FixedJoin[-L, -R, Out]

object FixedJoin {
  implicit def same[A]: FixedJoin[A, A, A] = new FixedJoin[A, A, A] {}
}

sealed trait Parent
final class LeftChild extends Parent
final class RightChild extends Parent

object Main {
  val joined: Join[None.type, None.type, Option[String]] =
    implicitly[Join[None.type, None.type, Option[String]]]
  val fixed: FixedJoin[None.type, None.type, Option[String]] =
    implicitly[FixedJoin[None.type, None.type, Option[String]]]
  val children: FixedJoin[LeftChild, RightChild, Parent] =
    implicitly[FixedJoin[LeftChild, RightChild, Parent]]

  def main(args: Array[String]): Unit = {
    println(joined != null)
    println(fixed != null)
    println(children != null)
  }
}
