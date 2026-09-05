// `Predef.identity` / `locally` / `implicitly` are `(A)A`, so with the real
// library on the classpath they are called through their erased descriptor
// `(Ljava/lang/Object;)Ljava/lang/Object;` -- and the result has to be
// coerced back, exactly as at any other erased call site. `gen_predef_poly`
// (crates/backend/src/gen_call.rs) emitted the call and nothing else, so a
// result stored into a field or selected from failed the JVM verifier:
//
//   VerifyError: Bad type on operand stack
//     Type 'java/lang/Object' is not assignable to 'Shape'
//
// It survived so long because the JVM verifier *does not check interface
// types*: `implicitly[SomeTrait].member` linked and ran with no cast at all.
// Only a value class -- a `class`, not a `trait` -- makes the missing cast
// observable, which is why every case below names one.
class Cell[A](val tag: String) {
  def show: String = "cell:" + tag
}

trait Level
trait Flat extends Level

// A class (not a trait) reached through a wildcard type argument: this is
// slick's `Shape[_ <: FlatShapeLevel, T, U, _]` shape, and the case that
// found the bug.
class Shape[L <: Level, T](val show: String)

object Shape {
  implicit def cellShape[L <: Level]: Shape[L, String] = new Shape("shape")
}

object Main {
  implicit val cs: Cell[String] = new Cell[String]("s")
  implicit val ci: Cell[Int] = new Cell[Int]("i")

  // A field initialiser: the `putfield` is what rejects an uncast `Object`.
  val fromImplicitly: Cell[String] = implicitly[Cell[String]]
  val inferred: Shape[_ <: Flat, String] = implicitly

  // `implicitly` at a bare type parameter must *not* be cast: the erasure of
  // `T` is `Object` and there is no class to name.
  def viaTypeParam[T](implicit t: T): T = implicitly[T]

  // ... but one at an applied class type must be.
  def viaClass[T](implicit c: Cell[T]): String = implicitly[Cell[T]].show

  def main(args: Array[String]): Unit = {
    println(fromImplicitly.show)
    println(inferred.show)
    println(implicitly[Cell[Int]].show)
    println(viaTypeParam[Cell[String]].show)
    println(viaClass[Int])

    // `identity` / `locally` take the same path. A primitive result is
    // unboxed, a `String` is cast to `String`, and a class result gets the
    // cast that was missing.
    val n: Int = identity(3)
    println(n)
    val l: Long = locally(7L)
    println(l)
    val s: String = identity("id")
    println(s)
    val c: Cell[Int] = identity(ci)
    println(c.show)
    val u: Unit = locally(())
    println(u)
    println(identity(implicitly[Cell[String]]).show)
  }
}
