// Method type parameters solved from the expected type as well as from the
// arguments (nsc's `instantiateExpecting`).

class Rep[T](val name: String)
class Inv[T](val name: String)

trait TypedType[T] { def label: String }

object TypedType {
  implicit val intType: TypedType[Int] = new TypedType[Int] { def label = "int" }
  implicit val strType: TypedType[String] = new TypedType[String] { def label = "str" }
  implicit val anyType: TypedType[Any] = new TypedType[Any] { def label = "any" }
}

object Library {
  // `T` shows up in no argument at all: only the expected type can pin it
  // down, and it has to be pinned down *before* the implicit clause runs.
  def column[T](name: String)(implicit tt: TypedType[T]): Rep[T] =
    new Rep[T](name + ":" + tt.label)

  // An invariant occurrence in the result: the expected type outranks the
  // argument, so `inv("q"): Inv[Any]` picks `T = Any`, not `T = String`.
  def inv[T](x: T)(implicit tt: TypedType[T]): Inv[T] = new Inv[T](tt.label)

  // A covariant occurrence is only an upper bound: the argument still wins,
  // so `cov("q"): List[Any]` keeps `T = String`.
  def cov[T](x: T)(implicit tt: TypedType[T]): List[T] = {
    println("cov " + tt.label)
    Nil
  }
}

object Main {
  def show(a: Array[AnyRef]): String = a.length.toString + "/" + a(0)

  def main(args: Array[String]): Unit = {
    // Array is invariant, so the expected type wins over the arguments:
    // T = AnyRef, and the array really is an Object[].
    val a: Array[AnyRef] = Array("x", "y")
    println(a.length)
    println(a(0))
    println(a.getClass.getName)
    println(show(a))

    val b: Array[Any] = Array(1, 2)
    println(b.length)
    println(b(1))
    println(b.getClass.getName)

    // An argument-driven Array keeps its own element type.
    val c = Array(3, 4)
    println(c.getClass.getName)
    println(c(1))

    // Expected type only.
    val r: Rep[Int] = Library.column("id")
    println(r.name)
    val s: Rep[String] = Library.column("nm")
    println(s.name)

    // Invariant result: the expected type wins over the argument.
    val i: Inv[Any] = Library.inv("q")
    println(i.name)
    // No expected type: the argument alone decides.
    val j = Library.inv(7)
    println(j.name)

    // Covariant result: the argument wins, `List[String] <: List[Any]`.
    val k: List[Any] = Library.cov("q")
    println(k)
  }
}
