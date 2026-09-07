// The cheap structural pre-test in front of implicit search
// (`Typer::plausibly_inhabits`, nsc's `isPlausiblyCompatible`) may only ever
// say "no" where the conformance check at the end of the fit says "no". The
// two things it must not get wrong are both here.
//
// 1. A witness whose result class is a strict *subclass* of the wanted one.
//    Deciding this needs the parent walk, not a comparison of head symbols:
//    `implicit val hex: Hex` is what answers a wanted `Digits`, and a filter
//    that rejected it would silently lose the witness.
//
// 2. A derivation rule that the wanted type constrains *only* through a `_`.
//    The `Shape` family below is slick's, written out with no jar and no
//    slick. Below the first level the wanted type is `Shape[_ <: L, M, U, P]`
//    with `M`, `U` and `P` unsolved, which binds only `Level` and binds it to
//    a wildcard -- that is, says nothing at all -- so such a rule is dropped.
//    A rule the wanted type pins down *anywhere else* must still be tried,
//    which is what every `mapped` call below is.
import scala.language.implicitConversions

trait Digits { def digits: String }
class Hex extends Digits { def digits = "0123456789abcdef" }

// Unrelated to `Digits`: the pre-test rejects it structurally, and it is here
// so that a regression which rejects too much is told apart from one that
// rejects too little.
class Roman { def digits = "IVXLCDM" }

trait Level
trait Flat extends Level

class Rep[T](val name: String)
class Shape[L <: Level, -M, U, P](val show: String)

object Shape {
  implicit def repShape[T, L <: Level]: Shape[L, Rep[T], T, Rep[T]] =
    new Shape("rep")

  implicit def tuple2Shape[L <: Level, M1, M2, U1, U2, P1, P2](implicit
      u1: Shape[_ <: L, M1, U1, P1],
      u2: Shape[_ <: L, M2, U2, P2]
  ): Shape[L, (M1, M2), (U1, U2), (P1, P2)] =
    new Shape("(" + u1.show + "," + u2.show + ")")

  implicit def tuple3Shape[L <: Level, M1, M2, M3, U1, U2, U3, P1, P2, P3](implicit
      u1: Shape[_ <: L, M1, U1, P1],
      u2: Shape[_ <: L, M2, U2, P2],
      u3: Shape[_ <: L, M3, U3, P3]
  ): Shape[L, (M1, M2, M3), (U1, U2, U3), (P1, P2, P3)] =
    new Shape("(" + u1.show + "," + u2.show + "," + u3.show + ")")
}

object Main {
  implicit val hex: Hex = new Hex
  implicit val roman: Roman = new Roman

  def summon[T](implicit e: T): T = e

  // `T` and `G` are the call site's, and only the witness can say what they
  // are -- the shape that made this family expensive in the first place.
  def mapped[F, T, G](f: F)(implicit s: Shape[_ <: Flat, F, T, G]): String =
    s.show

  def main(args: Array[String]): Unit = {
    println(summon[Digits].digits)
    println(summon[Roman].digits)
    println(mapped(new Rep[Int]("n")))
    println(mapped((new Rep[Int]("a"), new Rep[String]("b"))))
    println(mapped((new Rep[Int]("a"), new Rep[Int]("b"), new Rep[String]("c"))))
    // A *nested* tuple -- `mapped((r, (r, r)))` -- is accepted by real scalac
    // and is not found here, for a reason that predates this filter and is
    // written up on `Typer::implicit_fit_open`: `Unify` keys its unknowns by
    // symbol, so a rule applied inside itself has one `P1` where nsc has two
    // type variables, and the occurs check rejects `P1 := (P1, P2)`. It is
    // left out rather than asserted, so that gap is not mistaken for this one.
  }
}
