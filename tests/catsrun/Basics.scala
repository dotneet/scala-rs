// Functor / Applicative / Monad syntax and instances for the three shapes
// every cats program starts from: List, Option, Either.
import cats._
import cats.syntax.all._

object Main {
  def main(args: Array[String]): Unit = {
    println(Functor[List].map(List(1, 2, 3))(_ + 1))
    println(Functor[Option].map(Option(4))(_ * 2))
    println(Functor[Option].map(None: Option[Int])(_ * 2))

    println(Applicative[List].pure(7))
    println(Applicative[Option].pure("p"))
    println(Applicative[List].ap(List((i: Int) => i + 1, (i: Int) => i * 10))(List(1, 2)))

    println(Monad[Option].flatMap(Option(3))(i => Option(i + 1)))
    println(Monad[List].flatMap(List(1, 2))(i => List(i, i)))
    println(Monad[Option].flatMap(Option(3))(_ => None: Option[Int]))

    type E[A] = Either[String, A]
    println(Functor[E].map(Right(1): E[Int])(_ + 1))
    println(Functor[E].map(Left("boom"): E[Int])(_ + 1))
    println(Monad[E].flatMap(Right(2): E[Int])(i => Right(i * 3)))
    println(Monad[E].flatMap(Left("no"): E[Int])(i => Right(i * 3)))

    // syntax, not the instance methods
    println(List(1, 2, 3).map(_ + 1))
    println(Option(2).map(_ + 1))
    println(1.pure[Option])
    println(List(1, 2).flatMap(i => List(i, -i)))
    println((Option(1), Option(2)).mapN(_ + _))
    println((Option(1), None: Option[Int]).mapN(_ + _))
    println(Option(1).map2(Option(2))(_ + _))
    println(List(1, 2).map2(List(10, 20))(_ + _))

    // void / as / fproduct / tupleLeft
    println(Option(3).void)
    println(Option(3).as("x"))
    println(Option(3).fproduct(_ + 1))
    println(Option(3).tupleLeft("l"))
    println(List(1, 2).tupleRight("r"))

    // flatten, widen
    println(Option(Option(5)).flatten)
    println(List(List(1), List(2, 3)).flatten)
  }
}
