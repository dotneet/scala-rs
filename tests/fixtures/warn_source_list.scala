package scala.collection.immutable
sealed abstract class List[+A] {
  def ::[B >: A](head: B): List[B] = new ::(head, this)
  def keep: List[A] = {
    def loop(xs: List[A], lag: List[A]): List[A] = xs match {
      case _ :: tail => loop(tail, lag)
      case Nil => lag
    }
    loop(this, this)
  }
}
object List { def apply[A](): List[A] = Nil }
case object Nil extends List[Nothing]
final case class ::[A](head: A, tail: List[A]) extends List[A]
object Probe {
  def tail[A](xs: List[A]): List[A] = xs match {
    case _ :: rest => rest
    case Nil => Nil
  }
}
