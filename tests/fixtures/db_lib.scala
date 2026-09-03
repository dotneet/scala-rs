// agent/dbio: `Either.getOrElse` / `Try.getOrElse` の `[B1 >: B]`。
//
// prelude はどちらも `(=> Any): Any` と書いていたので、結果を使うたびに
// `… is not a member of Any` が出た(slick の
// `JdbcActionComponent.openStream` は 1 つの誤ったシグネチャから 3 件)。
// `Either` / `Try` は library ABI 専用なので、この fixture も jar モード限定。

import scala.util.{Failure, Success, Try}

class Rows[R](val xs: List[R]) {
  def first: R = xs.head
  def size: Int = xs.length
}

object Main {
  // 受け側 `Rows[T]` が呼び出し側の型パラメタを含む形(slick の
  // `PositionedResultIterator[T]`)。引数は `Nothing` なので、下限を使わない
  // と `B1` が `Nothing` に解ける。
  def firstRow[T](e: Either[Int, Rows[T]]): T =
    e.getOrElse(throw new NoSuchElementException("left")).first

  // 引数が下限より広い形: `B1` は `Any` ではなく `List[Any]`。
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
