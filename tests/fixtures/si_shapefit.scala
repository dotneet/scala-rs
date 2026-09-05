// A candidate whose own type parameters stand opposite a `_` in the wanted
// type is settled by its *own implicit clause*, not dropped.
//
// This is slick's `Shape` witness written out with no jar and no slick.
// `anyToShapedValue[T, U](value: T)(implicit shape: Shape[_ <: FlatShapeLevel,
// T, U, _]): ShapedValue[T, U]` is the conversion behind every `def * = (a, b)
// .mapTo[M]` in a slick table, and `tuple2Shape`'s `P1`/`P2` have nothing
// opposite them but that trailing `_`. `Unify` leaves them open, and a
// candidate with an open type parameter used to be dropped -- so the search
// failed even though the candidate's `u1`/`u2` say exactly what they are.
//
// `Shape.repShape` answers the leaves; `tuple2Shape` derives the pair from
// them and, in doing so, settles `U1`/`U2`/`P1`/`P2` and the conversion's own
// `U`. See `crates/cli/tests/slickimplicit.rs`.
import scala.language.implicitConversions

class Rep[T](val name: String)

trait ShapeLevel
trait FlatShapeLevel extends ShapeLevel

class Shape[Level <: ShapeLevel, -M, U, P](val show: String)

object Shape {
  implicit def repShape[T, L <: ShapeLevel]: Shape[L, Rep[T], T, Rep[T]] =
    new Shape("rep")

  implicit def tuple2Shape[L <: ShapeLevel, M1, M2, U1, U2, P1, P2](implicit
      u1: Shape[_ <: L, M1, U1, P1],
      u2: Shape[_ <: L, M2, U2, P2]
  ): Shape[L, (M1, M2), (U1, U2), (P1, P2)] =
    new Shape("(" + u1.show + "," + u2.show + ")")
}

class ShapedValue[T, U](val value: T, val shape: Shape[_ <: FlatShapeLevel, T, U, _]) {
  def describe: String = shape.show
}

object Conv {
  implicit def anyToShapedValue[T, U](value: T)(implicit
      shape: Shape[_ <: FlatShapeLevel, T, U, _]
  ): ShapedValue[T, U] = new ShapedValue(value, shape)
}

object Main {
  import Conv._

  val a = new Rep[String]("a")
  val b = new Rep[Int]("b")

  // `Predef.implicitly` would do, but selecting a member off its result emits
  // bytecode that does not verify (an unrelated backend gap); this is the same
  // search.
  def summon[T](implicit e: T): T = e

  // The wanted type pins `U`; `P1`/`P2` stand opposite the trailing `_`.
  val pair: Shape[_ <: FlatShapeLevel, (Rep[String], Rep[Int]), (String, Int), _] = summon

  // Nested, with every position pinned: the derivation runs two levels deep
  // and nothing is left open. (A nested one with the last position left `_`
  // is still not found -- see `crates/cli/tests/slickimplicit.rs`.)
  val nested: Shape[
    FlatShapeLevel,
    ((Rep[String], Rep[Int]), Rep[String]),
    ((String, Int), String),
    ((Rep[String], Rep[Int]), Rep[String])
  ] = summon

  def main(args: Array[String]): Unit = {
    println(pair.show)
    // Here `U` is undetermined too: nothing at the call site says what it is,
    // so it has to come out of the witness the clause finds.
    println(Conv.anyToShapedValue((a, b)).describe)
    // The same conversion used as a conversion, with the expected type
    // pinning `U`.
    val sv: ShapedValue[(Rep[String], Rep[Int]), (String, Int)] = (a, b)
    println(sv.describe)
    println(nested.show)
  }
}
