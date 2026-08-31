// Three causes behind slick's remaining `type mismatch`es, all of them plain
// language rules, so this runs on the private runtime as well as the jar.
//
//  * A type *constructor* parameter stands for its bound applied to the very
//    arguments the application passes: `M[A]` where `M[+X] <: Box[X]` is a
//    `Box[A]`. Only the bare `M` case was widened, so `m.head` came back as
//    `Box`'s own `X` and every use of the element was `found: A required: A`
//    -- two different symbols that print the same.
//  * A case class's companion `apply` was handed the *class's* type
//    parameters. One symbol then stood both for "fixed here" and "still to be
//    inferred at this call", so a call made from inside the class substituted
//    `U := U`, the parameter still mentioned a parameter of the callee, and
//    the argument was checked against its bound: `found: Bx[U] required:
//    Bx[Any]`.
//  * The implicit scope of a type is its companion *object*, and an object's
//    members include the ones it inherits. slick declares every `Shape` in a
//    trait and writes `object Shape extends ConstColumnShapeImplicits with …`,
//    so none of them were candidates at all. The wanted type's `_` is a
//    position the search is not asking about, and a contravariant parameter
//    means the wanted type is the *sub*type of what the candidate declares.

trait Box[+X] {
  def head: X
  def size: Int
}
final class One[+X](val head: X) extends Box[X] { def size = 1 }

trait Bx[X] { def name: String }

case class SV[T, U](a: T, b: Bx[U]) {
  // `SV.apply` seen from inside `SV` itself.
  def relabel(t: T): SV[T, U] = SV(t, b)
}

trait Lvl
trait Flat extends Lvl
abstract class Shp[L <: Lvl, -Mixed, Unpacked, Packed] { def tag: String }
class Rp[T]
class ConstCol[T] extends Rp[T]
class LitCol[T] extends ConstCol[T]

trait RepShapes {
  implicit def repShape[T, L <: Lvl]: Shp[L, Rp[T], T, Rp[T]] =
    new Shp[L, Rp[T], T, Rp[T]] { def tag = "rep" }
}
trait ConstShapes extends RepShapes {
  implicit def constShape[T, L <: Lvl]: Shp[L, ConstCol[T], T, ConstCol[T]] =
    new Shp[L, ConstCol[T], T, ConstCol[T]] { def tag = "const" }
}
object Shp extends ConstShapes

object Main {
  def firstOf[A, M[+X] <: Box[X]](m: M[A])(f: A => String): String =
    if (m.size == 0) "empty" else f(m.head)

  // `BP` is decided by the witness alone, and the witness has to be found
  // through a `_` and a contravariant `Mixed`.
  def packedTag[B, BP](b: B)(implicit shape: Shp[Flat, B, _, BP]): String = shape.tag

  def main(args: Array[String]): Unit = {
    println(firstOf(new One(7))(i => "n=" + (i + 1)))
    println(firstOf(new One("ab"))(s => "s=" + s.length))

    val sv = SV(1, new Bx[Boolean] { def name = "flag" })
    val re = sv.relabel(9)
    println(re.a.toString + "/" + re.b.name)

    println(packedTag(new LitCol[Boolean]))
    println(packedTag(new Rp[Int]))
  }
}
