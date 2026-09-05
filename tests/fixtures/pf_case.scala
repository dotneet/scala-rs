// Attributes the hand-written prelude used to drop that the library's own
// pickle carries. Every line here is accepted by scalac 2.13.16 and was
// rejected by scala-rs.
//
// 1. `Some`, `Left`, `Right`, `Success` and `Failure` are `case class`es --
//    `javap -p` shows `copy` and `copy$default$1` on each -- and the prelude
//    declared them without `Flags::CASE`, so `copy` was "not a member".
// 2. `::` was a second, empty class symbol beside `$colon$colon`, so it took
//    no type parameters and had no constructor.
// 3. A *qualified* constructor pattern took its class from a lexical lookup of
//    the last segment, so `Ior.Left(a)` found `scala.util.Left`.

import scala.util.{Failure, Success, Try}

object Main {
  // 1. `copy` on the library's own case classes.
  val some: Some[Int] = Some(1)
  val someCopied: Some[Int] = some.copy(value = 2)
  val somePositional: Some[Int] = some.copy(3)

  val left: Left[Int, String] = Left(1)
  val leftCopied: Left[Int, String] = left.copy(value = 2)
  val right: Right[Int, String] = Right("a")
  val rightCopied: Right[Int, String] = right.copy(value = "b")

  val success: Success[Int] = Success(1)
  val successCopied: Success[Int] = success.copy(value = 2)
  val failure: Failure[Int] = Failure[Int](new RuntimeException("boom"))
  val failureCopied: Failure[Int] = failure.copy(exception = new RuntimeException("bang"))

  // 2. `::` names the cons cell, with its type parameter and its constructor.
  val cons: ::[Int] = new ::(1, Nil)
  def consHead(c: ::[Int]): Int = c.head

  // 3. A nested case class whose simple name collides with a prelude class.
  sealed abstract class Ior[+A, +B] {
    def fold[C](fa: A => C, fb: B => C, fab: (A, B) => C): C = this match {
      case Ior.Left(a)    => fa(a)
      case Ior.Right(b)   => fb(b)
      case Ior.Both(a, b) => fab(a, b)
    }
  }
  object Ior {
    final case class Left[+A](a: A) extends Ior[A, Nothing]
    final case class Right[+B](b: B) extends Ior[Nothing, B]
    final case class Both[+A, +B](a: A, b: B) extends Ior[A, B]
  }

  // The prelude's own classes still destructure through their extractors.
  def describe(e: Either[Int, String]): String = e match {
    case Left(i)  => "L" + i
    case Right(s) => "R" + s
  }
  def describeTry(t: Try[Int]): String = t match {
    case Success(v) => "S" + v
    case Failure(e) => "F" + e.getMessage
  }

  def main(args: Array[String]): Unit = {
    println(someCopied)
    println(somePositional)
    println(leftCopied)
    println(rightCopied)
    println(successCopied)
    println(failureCopied.exception.getMessage)
    println(consHead(cons))
    println(cons.tail.isEmpty)
    val a: Ior[Int, String] = Ior.Left(7)
    val b: Ior[Int, String] = Ior.Right("x")
    val c: Ior[Int, String] = Ior.Both(1, "y")
    println(a.fold(_.toString, s => s, (i, s) => s + i))
    println(b.fold(_.toString, s => s, (i, s) => s + i))
    println(c.fold(_.toString, s => s, (i, s) => s + i))
    println(describe(Left(1)))
    println(describe(Right("z")))
    println(describeTry(Success(4)))
    println(describeTry(Failure(new RuntimeException("m"))))
  }
}
