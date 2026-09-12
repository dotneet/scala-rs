// EitherT / OptionT / Kleisli -- the monad transformers, where the Monad of the
// inner effect has to be threaded through every method.
import cats._
import cats.data._
import cats.syntax.all._

object Main {
  type Id2[A] = A

  def main(args: Array[String]): Unit = {
    // OptionT over List
    val ot: OptionT[List, Int] = OptionT(List(Option(1), None, Option(3)))
    println(ot.value)
    println(ot.map(_ + 1).value)
    println(ot.flatMap(i => OptionT.pure[List](i * 10)).value)
    println(ot.getOrElse(0))
    println(ot.isDefined)
    println(OptionT.liftF(List(1, 2)).value)
    println(OptionT.none[List, Int].value)
    println(OptionT.some[List](5).value)
    println(OptionT.fromOption[List](Option(9)).value)

    // OptionT over Either
    type E[A] = Either[String, A]
    val oe: OptionT[E, Int] = OptionT(Right(Option(4)): E[Option[Int]])
    println(oe.map(_ * 2).value)
    println(oe.flatMapF(i => Right(Option(i + 1)): E[Option[Int]]).value)
    println(Monad[OptionT[E, *]].flatMap(oe)(i => OptionT.pure[E](i + 100)).value)

    // EitherT over Option
    val et: EitherT[Option, String, Int] = EitherT(Option(Right(2): Either[String, Int]))
    println(et.value)
    println(et.map(_ + 1).value)
    println(et.leftMap(_ + "!").value)
    println(et.flatMap(i => EitherT.pure[Option, String](i * 5)).value)
    println(EitherT.leftT[Option, Int]("bad").value)
    println(EitherT.rightT[Option, String](3).value)
    println(EitherT.liftF[Option, String, Int](Option(8)).value)
    println(et.getOrElse(0))
    println(EitherT.fromEither[Option](Left("l"): Either[String, Int]).value)
    println(et.fold(l => "L" + l, r => "R" + r))

    // EitherT over List, with a left that short-circuits
    val el: EitherT[List, String, Int] = EitherT(List(Right(1), Left("e"), Right(3)))
    println(el.value)
    println(el.map(_ * 2).value)
    println(el.semiflatMap(i => List(i, i)).value)

    // Kleisli
    val k: Kleisli[Option, Int, String] = Kleisli(i => Option(i.toString))
    println(k.run(7))
    println(k.map(_ + "!").run(3))
    println(k.flatMap(s => Kleisli[Option, Int, String](i => Option(s + i))).run(2))
    println(k.local[Int](_ * 10).run(4))
    println(Kleisli.pure[Option, Int, String]("p").run(0))
    println(Kleisli.liftF[Option, Int, String](Option("lf")).run(0))
    println(Kleisli.ask[Option, Int].run(11))
    val k2: Kleisli[Option, String, Int] = Kleisli(s => Option(s.length))
    println(k.andThen(k2).run(1234))
    println(Monad[Kleisli[Option, Int, *]].flatMap(k)(s => Kleisli(i => Option(s + ":" + i))).run(5))

    // Kleisli over Id
    val ki: Kleisli[Id2, Int, Int] = Kleisli[Id2, Int, Int](i => i + 1)
    println(ki.run(1))
    println(ki.map(_ * 2).run(1))

    // Writer / Reader-ish, via Kleisli and WriterT
    val w: WriterT[Option, List[String], Int] = WriterT(Option((List("start"), 1)))
    println(w.run)
    println(w.map(_ + 1).run)
    println(w.tell(List("more")).run)
    println(w.written)
    println(w.value)
  }
}
