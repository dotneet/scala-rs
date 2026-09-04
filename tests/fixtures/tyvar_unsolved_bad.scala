// Pins that undetermined type variables are never "treated as solved". Real scalac
// 2.13.16 rejects all of the below too (with different wording).

object WrongClass {
  def take(m: Map[String, Int]): Int = m.size
  // scalac: polymorphic expression cannot be instantiated to expected type;
  //         found [A]List[A]  required Map[String,Int]
  val bad = take(List.empty)
}

object WrongClass2 {
  def take(v: Vector[String]): Int = v.length
  val bad = take(Map.empty)
}

// An enclosing method's type parameter is a fixed type, not an "undetermined
// variable". It must not be solved to suit a parameter type.
object EnclosingTparamIsNotAVariable {
  def take(m: Map[String, Int]): Int = m.size
  def g[K](m: Map[K, Int]): Int = take(m)
}

// An argument's type variable is solvable only when the parameter type it is passed
// to can solve it. `Vector[A]` does not meet `Map[T, Int]`.
object NoShapeMatch {
  def f[T](x: T, m: Map[T, Int]): Int = m.size
  def g[T](x: T): Int = f(x, Vector.empty)
}

// A shape nsc also turns down, saying "undetermined type". A function literal in a
// tuple element cannot be typed until `Tuple2.apply`'s type parameters are decided.
object UndeterminedInTuple {
  def f[A, B](p: (A, B => Int)): Int = 1
  val bad = f(("x", n => n + 1))
}
