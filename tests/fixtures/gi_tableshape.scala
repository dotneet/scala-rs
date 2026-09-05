// A candidate whose own type parameter is settled by an *evidence* clause,
// where the wanted type only ever equates it with a type parameter the call
// site has not settled either.
//
// This is slick's other `Shape` witness, written out with no jar and no slick.
// `AbstractTable.tableShape[Level, T, C <: AbstractTable[_]](implicit ev: C
// <:< AbstractTable[T]): Shape[Level, C, T, C]` is what answers `q.map(a => a)`
// and every tuple that mentions a table, and its `T` appears on the wanted
// side only opposite `map`'s own undetermined `T`. Unifying the result solved
// nothing about it, so the search asked for `Accounts <:< AbstractTable[T]`
// with `T` free and found no witness -- while `ev` says exactly what it is,
// through the *base type* of `Accounts` at `AbstractTable`.
//
// `Ev` stands in for `scala.<:<` so this runs without the library jar too; the
// variance is what matters, since `refl[A]: Ev[A, A]` answers
// `Ev[Accounts, AbstractTable[?T]]` only by widening `Accounts` to its base
// type at `AbstractTable`.
import scala.language.implicitConversions

class Rep[T](val name: String)

trait ShapeLevel
trait FlatShapeLevel extends ShapeLevel

class Shape[Level <: ShapeLevel, -M, U, P](val show: String)

class Ev[-From, +To]
object Ev {
  implicit def refl[A]: Ev[A, A] = new Ev
}

abstract class AbstractTable[T](val label: String)
class Accounts extends AbstractTable[(String, Int)]("accounts")
class Labels extends AbstractTable[String]("labels")

object Shape {
  implicit def repShape[T, L <: ShapeLevel]: Shape[L, Rep[T], T, Rep[T]] =
    new Shape("rep")

  implicit def tableShape[L <: ShapeLevel, T, C <: AbstractTable[_]](implicit
      ev: Ev[C, AbstractTable[T]]
  ): Shape[L, C, T, C] = new Shape("table")

  implicit def tuple2Shape[L <: ShapeLevel, M1, M2, U1, U2, P1, P2](implicit
      u1: Shape[_ <: L, M1, U1, P1],
      u2: Shape[_ <: L, M2, U2, P2]
  ): Shape[L, (M1, M2), (U1, U2), (P1, P2)] =
    new Shape("(" + u1.show + "," + u2.show + ")")
}

object Main {
  def summon[T](implicit e: T): T = e

  // `Query.map`'s shape: `T` and `G` are the call site's, and only the witness
  // can say what they are.
  def mapped[F, T, G](f: F)(implicit s: Shape[_ <: FlatShapeLevel, F, T, G]): String =
    s.show

  // With every position pinned, so the solution can be read back.
  val direct: Shape[_ <: FlatShapeLevel, Accounts, (String, Int), Accounts] = summon
  val inTuple: Shape[
    FlatShapeLevel,
    (Accounts, Rep[Int]),
    ((String, Int), Int),
    (Accounts, Rep[Int])
  ] = summon

  def main(args: Array[String]): Unit = {
    println(mapped(new Accounts))
    println(mapped(new Labels))
    println(mapped((new Accounts, new Rep[Int]("n"))))
    println(mapped((new Labels, new Accounts)))
    println(direct.show)
    println(inTuple.show)
  }
}
