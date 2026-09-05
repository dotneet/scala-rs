// An argument that already conforms must not be wrapped in a view first.
//
// `docs/gitbucket.md`'s "what would remove the most next", entry 5. For
//
//   class Rep[T]; class Lit[T](v: T) extends Rep[T]
//   def ===[P2, R](e: Rep[P2])(implicit om: OM[B1, P2, R]): String
//
// `col === new Lit[Long](9L)` solves `P2 = Long` in nsc, because `Lit[Long]`
// *is* a `Rep[Long]` through its base type. We solved `P2 = Lit[Long]`: the
// applicability test read the still-unsolved `P2` as a rigid type, decided the
// argument did not fit, and `apply_open_views` reached for the `T => Rep[T]`
// view that is in scope for the sake of the literal on the next line. The
// wrapping is what the call's inference then read, so the implicit clause
// asked for `OM[Long, Lit[Long], R]`, which nothing supplies -- a *wrong*
// answer rather than a missing one, which is why everything downstream of it
// broke too.
//
// The rule is nsc's `isCompatible` under undetermined type variables: a view
// is inserted only where the argument does not already fit for *some*
// instantiation of what the call has not settled yet.
//
// Real scalac 2.13.16 compiles this file and prints the same output.
import scala.language.implicitConversions

class Rep[T](val label: String)
class Lit[T](v: T) extends Rep[T]("lit:" + v)
// A subclass whose own type parameters do not line up with the parent's:
// positional unification cannot answer this one, only the base type can.
class Tagged[A, B](a: A) extends Rep[B]("tag:" + a)
// And one with no type parameters at all.
class LongLit extends Rep[Long]("fixed")

trait OM[B1, P2, R] { def name: String }

class Col[B1](val col: String) {
  // slick's shape: `R` appears only in the implicit clause and the result.
  def ===[P2, R](e: Rep[P2])(implicit om: OM[B1, P2, R]): String =
    col + " = " + e.label + " via " + om.name
}

object Main {
  implicit def toRep[T](v: T): Rep[T] = new Lit[T](v)
  implicit val omLong: OM[Long, Long, Boolean] = new OM[Long, Long, Boolean] {
    def name = "omLong"
  }
  implicit val omStr: OM[String, String, Int] = new OM[String, String, Int] {
    def name = "omStr"
  }

  // The same question with the type parameter in the implicit clause only.
  def only[P](e: Rep[P])(implicit om: OM[Long, P, Boolean]): String = om.name

  def main(args: Array[String]): Unit = {
    val id = new Col[Long]("ID")
    val nm = new Col[String]("NAME")
    // A subclass argument: `Lit[Long] <: Rep[Long]`, so no view is wanted.
    println(id === new Lit[Long](9L))
    // The base type is the only thing that says `P2 = Long` here.
    println(id === new Tagged[String, Long]("x"))
    println(id === new LongLit)
    // The view really is needed for these two, and still fires.
    println(id === 1L)
    println(nm === "bob")
    // Exactly the parameter's class: unchanged.
    println(id === new Rep[Long]("nine"))
    // A type parameter that only the implicit clause mentions.
    println(only(new Lit[Long](3L)))
    println(only(new LongLit))
  }
}
