// Option, Either and Try combinators, including laziness of getOrElse's
// argument, Option(null), Either projections and Try recovery.
import scala.util.{Try, Success, Failure}
object Main {
  var sideEffects = 0
  def bump(): Int = { sideEffects += 1; -1 }
  def parse(s: String): Either[String, Int] = Try(s.toInt).toEither.left.map(_ => "bad:" + s)
  def main(args: Array[String]): Unit = {
    val some: Option[Int] = Some(3); val none: Option[Int] = None
    println(some.getOrElse(bump()) + " " + none.getOrElse(bump()) + " effects=" + sideEffects)
    println(some.map(_ * 2) + " " + none.map(_ * 2) + " " + some.flatMap(x => if (x > 2) Some(x) else None) + " " + some.filter(_ > 5))
    println(some.fold("empty")(_.toString) + " " + none.fold("empty")(_.toString) + " " + some.orElse(Some(9)) + " " + none.orElse(Some(9)))
    println(Option(null: String) + " " + Option("x") + " " + some.contains(3) + " " + some.exists(_ > 1) + " " + none.forall(_ > 1))
    println(some.toList ++ none.toList + " " + some.zip(Some("s")) + " " + some.collect { case 3 => "three" } + " " + none.isEmpty)
    println(some.iterator.toList + " " + Some(Some(1)).flatten + " " + (None: Option[String]).orNull)
    val intOrNull: Option[java.lang.Integer] = None
    println(intOrNull.orNull)
    println(List("1", "x", "3").map(parse))
    val r: Either[String, Int] = Right(5); val l: Either[String, Int] = Left("no")
    println(r.map(_ + 1) + " " + l.map(_ + 1) + " " + r.flatMap(x => Left("fail" + x)) + " " + l.getOrElse(0) + " " + r.getOrElse(0))
    println(r.swap + " " + l.left.getOrElse("?") + " " + r.fold(_.length, _ * 10) + " " + l.fold(_.length, _ * 10) + " " + r.isRight + " " + l.isLeft)
    println(r.toOption + " " + l.toOption + " " + r.filterOrElse(_ > 10, "small") + " " + r.contains(5))
    val eithers = List(Right(1), Left("a"), Right(2), Left("b"))
    println(eithers.collect { case Right(v) => v } + " " + eithers.partitionMap(identity))
    val t1 = Try(10 / 2); val t2 = Try(10 / 0); val t3 = Try("x".toInt)
    println(t1 + " " + t2.isFailure + " " + t3.getClass.getSimpleName)
    println(t2.recover { case _: ArithmeticException => -1 } + " " + t3.getOrElse(0) + " " + t1.map(_ + 1) + " " + t2.toOption)
    t2 match { case Failure(e) => println("failure " + e.getClass.getSimpleName + ": " + e.getMessage); case Success(v) => println(v) }
    println(Try(throw new IllegalStateException("boom")).failed.map(_.getMessage))
    println(t1.flatMap(v => Try(v * 3)) + " " + t1.filter(_ > 100).isFailure + " " + t3.recoverWith { case _ => Success(7) })
    println(Try { sideEffects += 100; sideEffects }.get)
    val fe = for { a <- parse("4"); b <- parse("5") } yield a * b
    val fe2 = for { a <- parse("4"); b <- parse("q") } yield a * b
    println(fe + " " + fe2)
    val fo = for { a <- some; b <- Option(2) if a > b } yield a - b
    println(fo)
  }
}
