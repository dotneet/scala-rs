// agent/dbio: the `[B1 >: B]` of `Either.getOrElse` / `Try.getOrElse`.
//
// The prelude wrote both as `(=> Any): Any`, so every use of the result drew
// a `… is not a member of Any` (slick's `JdbcActionComponent.openStream` got
// three of them out of one wrong signature).
// `Either` / `Try` are library-ABI only, so this fixture is jar mode only.

import scala.util.{Failure, Success, Try}

class Rows[R](val xs: List[R]) {
  def first: R = xs.head
  def size: Int = xs.length
}

object Main {
  // The receiver `Rows[T]` mentions the caller's type parameter (slick's
  // `PositionedResultIterator[T]`). The argument is `Nothing`, so without the
  // lower bound `B1` solves to `Nothing`.
  def firstRow[T](e: Either[Int, Rows[T]]): T =
    e.getOrElse(throw new NoSuchElementException("left")).first

  // The argument is wider than the lower bound: `B1` is `List[Any]`, not `Any`.
  def widened(e: Either[Int, List[Int]]): List[Any] = e.getOrElse(List("s"))

  def tried[T](t: Try[Rows[T]]): Int = t.getOrElse(new Rows[T](Nil)).size

  def main(args: Array[String]): Unit = {
    println(firstRow(Right(new Rows(List("a", "b")))))
    println(widened(Right(List(1, 2))))
    println(widened(Left(0)))
    println(tried(Success(new Rows(List(1, 2, 3)))))
    println(tried(Failure[Rows[Int]](new RuntimeException("boom"))))
  }
}
