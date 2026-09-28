// An implicit whose only clause is implicit is value-shaped: nsc drops the
// clause before comparing specificity, so `opt` (a `Show[Option[T]]`) is
// more specific than `derive` (a `Show[T]`) for a `Show[Option[Int]]`.
package implspec

trait Show[T] { def show(t: T): String }
object Show {
  implicit val int: Show[Int] = (t: Int) => "i" + t
  implicit def opt[T](implicit s: Show[T]): Show[Option[T]] =
    (t: Option[T]) => t.fold("none")(s.show)
  implicit def list[T](implicit s: Show[T]): Show[List[T]] =
    (t: List[T]) => t.map(s.show).mkString("[", ",", "]")
  implicit def derive[T]: Show[T] = (t: T) => "derived"
}

final case class P(a: Int)

object Main {
  def main(args: Array[String]): Unit = {
    println(implicitly[Show[Option[Int]]].show(Some(1)))
    println(implicitly[Show[List[Option[Int]]]].show(List(Some(2), None)))
    // Only `derive` fits a case class.
    println(implicitly[Show[P]].show(P(3)))
  }
}
