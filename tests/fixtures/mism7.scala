// Seventh `type mismatch` slice.
//
// Everything here runs on both the private runtime and the real
// scala-library, so it names only `List`, `Option`, `Tuple2` and `String` --
// and prints scalars, because the private runtime's classes do not override
// `toString`. The jar-only cases (`toMap`, `indexWhere`, `Vector`) are in
// crates/cli/tests/mismatch7.rs.

// A method's parameter seen from an anonymous class of the *same* generic
// class: `f` is owned by `map`, not by `It`, so the anonymous class's parent
// `It[B]` must not substitute `T := B` into its type.
trait It[T] { self =>
  def next(): T
  def map[B](f: T => B): It[B] = new It[B] {
    def next(): B = f(self.next())
  }
}

// `A with B` is a legal *type* even for two unrelated classes; only a
// template may not mix a second class in.
class Ca
class Cb
trait Tb

class Inv[T](val t: T)

class Rep[T]
trait SE[T, U]

object Fwd {
  // A forward reference to a companion's `apply` whose result type is
  // inferred: the module -> `apply` redirect has to complete the signature.
  val one: SE[Rep[Int], Int] = SE[Rep[Int], Int]
}

object SE extends SE[Rep[Any], Any] {
  def apply[T <: Rep[?], U] = this.asInstanceOf[SE[T, U]]
}

// `K` sits in no explicit parameter: only the witness can say what it is, and
// filling the clause is what turns `w.conv` into a value.
class Conv[A, K]
object Conv {
  implicit val stringToInt: Conv[String, Int] = new Conv[String, Int]
}
class Res[K](val n: Int)
class Wrap[A](val a: A) {
  def conv[K](implicit ev: Conv[A, K]): Res[K] = new Res[K](1)
}

object Main {
  def firstOf[E, O >: E](x: E): O = x

  def widen[E, O >: E](x: Inv[E]): Inv[? <: O] = x

  def force[T](xs: List[T]): List[T] = xs.map(identity)

  // The explicit `_` form solves the method's type parameters the same way.
  val etaId: String => String = identity _

  def keep[X](x: X): X = x
  def viaParam(w: Wrap[String]): Int = keep(w.conv).n
  def inTuple(w: Wrap[String]): (Int, Res[Int]) = (1, w.conv)

  def mixed(x: Ca with Tb): Boolean = x != null

  // Two *classes* in one compound type: a legal signature, even though no
  // value can inhabit it, so nothing calls it.
  def twoClasses(x: Ca with Cb): Int = 1

  // Two `Inv`s of an *invariant* class in one list: the element type is the
  // existential `Inv[_ <: Any]`, not `Inv[Any]`, which neither of them
  // conforms to. (`List(a, b)` says the same through varargs; the private
  // runtime has no `List.apply`, so that one is in mismatch7.rs.)
  def invs: List[Inv[?]] = new Inv(1) :: new Inv("s") :: Nil

  def main(args: Array[String]): Unit = {
    val it = new It[String] {
      private var n = 0
      def next(): String = { n += 1; "n" + n.toString }
    }
    val strs = it.map(i => i + "!")
    println(strs.next())
    println(strs.next())

    println(firstOf[Int, Any](7).toString)
    println(widen[Ca, Any](new Inv(new Ca)).t.isInstanceOf[Ca])

    println(force(1 :: 2 :: Nil).isEmpty)
    println(etaId("a"))

    println(viaParam(new Wrap("s")))
    println(inTuple(new Wrap("s"))._2.n)

    println(invs.isEmpty)
    println(Fwd.one != null)
    println(mixed(new Ca with Tb))
  }
}
