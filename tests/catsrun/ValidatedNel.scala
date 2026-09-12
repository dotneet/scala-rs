// Validated accumulating errors through NonEmptyList -- the reason most
// programs reach for cats in the first place.
import cats._
import cats.data._
import cats.syntax.all._

case class Form(name: String, age: Int, mail: String)

object Main {
  def checkName(s: String): ValidatedNel[String, String] =
    if (s.nonEmpty) s.validNel else "name is empty".invalidNel

  def checkAge(i: Int): ValidatedNel[String, Int] =
    if (i >= 0 && i < 150) i.validNel else s"age out of range: $i".invalidNel

  def checkMail(s: String): ValidatedNel[String, String] =
    if (s.contains("@")) s.validNel else s"not an address: $s".invalidNel

  def validate(n: String, a: Int, m: String): ValidatedNel[String, Form] =
    (checkName(n), checkAge(a), checkMail(m)).mapN(Form.apply)

  def main(args: Array[String]): Unit = {
    println(validate("ada", 36, "ada@example.com"))
    println(validate("", 200, "nope"))
    println(validate("", 36, "ada@example.com"))

    val v1: ValidatedNel[String, Int] = "bad1".invalidNel
    val v2: ValidatedNel[String, Int] = "bad2".invalidNel
    println((v1, v2).mapN(_ + _))
    println((v1, 3.validNel[String]).mapN(_ + _))
    println(v1.combine(v2))

    // the Applicative instance by hand
    println(Applicative[ValidatedNel[String, *]].map2(v1, v2)(_ + _))
    println(Semigroupal[ValidatedNel[String, *]].product(v1, v2))

    // NonEmptyList in its own right
    val nel = NonEmptyList.of(3, 1, 2)
    println(nel)
    println(nel.head)
    println(nel.tail)
    println(nel.sorted)
    println(nel.reverse)
    println(nel.map(_ * 2))
    println(nel.toList)
    println(nel.reduceLeft(_ + _))
    println(nel ++ List(9))
    println(NonEmptyList.fromList(List(1, 2)))
    println(NonEmptyList.fromList(Nil: List[Int]))
    println(NonEmptyList.one(7))
    println(Semigroup[NonEmptyList[Int]].combine(nel, NonEmptyList.one(8)))
    println(Functor[NonEmptyList].map(nel)(_ + 1))
    println(Traverse[NonEmptyList].traverse(nel)(i => Option(i + 1)))
    println(Reducible[NonEmptyList].reduceLeft(nel)(_ + _))

    // Validated <-> Either
    println(v1.toEither)
    println(3.validNel[String].toEither)
    println(Validated.fromEither[String, Int](Left("l")))
    println(Validated.fromEither[String, Int](Right(2)))
    println(v1.fold(e => "E" + e.toList.mkString(","), i => "A" + i))
  }
}
