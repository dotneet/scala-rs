// An implicit view that makes an argument applicable, where the callee has a
// type parameter the argument's parameter type does not mention.
//
// `search_conversion_open` demanded a solution for *every* one of the callee's
// type parameters before it would offer a view. slick's
//
//   def === [P2, R](e: Rep[P2])(implicit om: OptionMapper2[B1, B1, Boolean, P1, P2, R]): Rep[R]
//
// is the shape that breaks: `Rep[P2]` says nothing about `R`, which only the
// implicit clause and the result mention, so `column === 1L` could not reach
// the `Long => Rep[Long]` view that makes the call applicable at all, and came
// out `no matching overload for (Rep[P2])(OptionMapper2[...])Rep[R] with
// arguments (1L)`. Only the type parameters the *wanted* type mentions can be
// settled by that unification.
//
// Real scalac 2.13.16 compiles this file and prints the same output.
import scala.language.implicitConversions

class Rep[T](val label: String)
class Lit[T](v: T) extends Rep[T]("lit:" + v)
trait OM[B1, P2, R] { def name: String }

class Col[B1](val col: String) {
  // The slick shape: `R` appears only in the implicit clause and the result.
  def ===[P2, R](e: Rep[P2])(implicit om: OM[B1, P2, R]): String =
    col + " = " + e.label + " via " + om.name

  // The same, with the argument's parameter type mentioning both.
  def in[P2, R](e: Rep[P2], r: R)(implicit om: OM[B1, P2, R]): String =
    col + " in " + e.label + " " + r + " via " + om.name
}

object Main {
  implicit def toRep[T](v: T): Rep[T] = new Lit[T](v)
  implicit val omLong: OM[Long, Long, Boolean] = new OM[Long, Long, Boolean] {
    def name = "omLong"
  }
  implicit val omStr: OM[String, String, Int] = new OM[String, String, Int] {
    def name = "omStr"
  }

  def main(args: Array[String]): Unit = {
    val id = new Col[Long]("ID")
    val nm = new Col[String]("NAME")
    // The literal reaches `Rep[P2]` only through `toRep`.
    println(id === 1L)
    println(nm === "bob")
    // A value, not a literal: the same path without constant types.
    val v: Long = 7L
    println(id === v)
    // Already a `Rep`: no view needed, and the answer must not change.
    println(id === new Rep[Long]("nine"))
    // A parameter type that mentions both type parameters still works.
    println(nm.in("x", 3))
  }
}
