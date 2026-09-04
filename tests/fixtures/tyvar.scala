// Deferred resolution of type variables (nsc's undetermined type variables).
//
// Arguments are typed without an expected type so that overload resolution can run,
// so a polymorphic reference like `Map.empty` reaches argument position still
// carrying its own type parameters (`Map[K, V]`). nsc carries those around as
// "undetermined type variables" and solves them all at once against the parameter
// types, after it has picked a candidate for the call. Everything below is a shape
// real scalac 2.13.16 accepts.

object Empties {
  def m(m: Map[String, Int]): Int = m.size
  def v(v: Vector[String]): Int = v.length
  def s(s: Set[Int]): Int = s.size
  def l(xs: List[Int], ys: List[Int]): Int = xs.size + ys.size
  def sq(xs: Seq[String]): Int = xs.length

  val a = m(Map.empty)
  val b = v(Vector.empty)
  val c = s(Set.empty)
  val d = l(List.empty, Nil)
  val e = sq(Seq.empty)
}

// An empty `apply` (`Map()` / `Vector()`) has the same shape: the result type comes
// back still carrying its own type parameters.
object EmptyApplies {
  def m(m: Map[String, Int]): Int = m.size
  def v(v: Vector[String]): Int = v.length
  def l(xs: List[String]): Int = xs.length

  val a = m(Map())
  val b = v(Vector())
  val c = l(List())
}

// Undetermined type variables leak out of nested calls too. `id`'s `T` solves to
// `Map[K, V]`, but `K` and `V` are decided by the outer parameter type.
object Nested {
  def id[T](x: T): T = x
  def take(m: Map[String, Int]): Int = m.size
  val a = take(id(Map.empty))
}

// A variable that reaches the result type is decided by the expected type. The
// result of `f(Map.empty)` is `List[Map[?K, ?V]]`, and the declared
// `List[Map[String, Int]]` decides `?K` / `?V`.
object FromExpected {
  def f[T](x: T): List[T] = List(x)
  val a: List[Map[String, Int]] = f(Map.empty)
}

// Varargs, by-name and default-argument positions take the same path.
object OtherPositions {
  def varargs(xs: Map[String, Int]*): Int = xs.length
  def byName(m: => Map[String, Int]): Int = m.size
  def withDefault(m: Map[String, Int] = Map.empty): Int = m.size

  val a = varargs(Map.empty, Map.empty)
  val b = byName(Map.empty)
  val c = withDefault()
  val d = withDefault(Map.empty)
}

// The same holds with several arguments, and with several clauses.
object Clauses {
  def two(n: Int, m: Map[String, Int]): Int = n + m.size
  def curried(m: Map[String, Int])(n: Int): Int = m.size + n
  val a = two(1, Map.empty)
  val b = curried(Map.empty)(2)
}

// The overload choice itself is made through undetermined type variables.
object Overloaded {
  def f(x: Seq[Int]): Int = x.sum
  def f(x: String): Int = x.length
  val a = f(Seq.empty)
}

// Constructor arguments go down the same path.
class Box(val m: Map[String, Int], val v: Vector[String]) {
  def size: Int = m.size + v.length
}

object Ctor {
  val a = new Box(Map.empty, Vector.empty).size
}

// When it is the caller's type parameter that is undetermined (the other half of
// nsc's undetparams). `xs.collect { case … }` is checked against
// `PartialFunction[Int, ?B]`, and `?B` is decided by the literal's result type.
// Collapsing it to `Any` here wrecks the result type, so we use the argument's solution.
object CalleeOpen {
  val xs = List(1, 2, 3, 4)
  val a: List[String] = xs.collect { case n if n % 2 == 0 => n.toString }
  val b: List[Int] = xs.map(n => n + 1)
  val c: Option[Int] = Some(3).collect { case n => n * 2 }
}

object Main {
  def main(args: Array[String]): Unit = {
    println(Empties.a)
    println(Empties.b)
    println(Empties.c)
    println(Empties.d)
    println(Empties.e)
    println(EmptyApplies.a)
    println(EmptyApplies.b)
    println(EmptyApplies.c)
    println(Nested.a)
    println(FromExpected.a)
    println(OtherPositions.a)
    println(OtherPositions.b)
    println(OtherPositions.c)
    println(OtherPositions.d)
    println(Clauses.a)
    println(Clauses.b)
    println(Overloaded.a)
    println(Ctor.a)
    println(CalleeOpen.a)
    println(CalleeOpen.b)
    println(CalleeOpen.c)
  }
}
