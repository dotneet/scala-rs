// agent/tq: slick's `TableQuery` / `Compiled` shapes.
//
// Three roots, one file:
//
//  (A) the application of an *abstract* type constructor (`C[BU]`, `C[_]` a
//      type parameter) under a wildcard argument -- `Rep[C[BU]] <: Rep[_]`;
//  (B) an overloaded `apply` selected with explicit type arguments and then
//      applied, where one alternative is parameterless;
//  (C) an implicit whose own type parameters only its *own* implicit clause
//      can pin down, because the call site's output type is undetermined.

// ---------------------------------------------------------------- (A) ----

trait Rep[T] { def label: String }
trait QueryBase[T] extends Rep[T]
trait Query[+E, U, C[_]] extends QueryBase[C[U]]

trait Exec[T, TU, EU] { def name: String }

object Exec extends Exec[Rep[Any], Any, Any] {
  def name = "exec"
  // `T <: Rep[_]`: checking `Query[B, BU, C]` against it walks to
  // `Rep[C[BU]]` and then asks `C[BU] <: _`.
  def apply[T <: Rep[_], TU, EU] = this.asInstanceOf[Exec[T, TU, EU]]
}

class SeqQuery[E, U](val label: String, val elem: E) extends Query[E, U, Seq]

object Wild {
  def queryExec[B, BU, C[_]]: Exec[Query[B, BU, C], C[BU], BU] =
    Exec[Query[B, BU, C], C[BU], BU]
  // The same conformance without a bound check in the way.
  def asRep[B, BU, C[_]](q: Query[B, BU, C]): Rep[_] = q
  def asRepAt[B, BU](q: Query[B, BU, Seq]): Rep[_] = q
}

// ---------------------------------------------------------------- (B) ----

class TQ[E](val cons: Int => E) { def head: E = cons(0) }

object TQ {
  def apply[E](cons: Int => E): TQ[E] = new TQ[E](cons)
  def apply[E]: TQ[E] = new TQ[E](_ => null.asInstanceOf[E])
}

// A *parameterless* polymorphic method whose result carries the `apply` that
// takes the arguments (fs2's `Stream.fromIterator[F]`). The type arguments
// belong to the method; the value arguments -- one of them named -- to the
// result's `apply`.
class Partial[F](val tag: String) {
  def apply[A](x: A, n: Int): String = tag + "(" + x + "," + n + ")"
}

object Fac {
  def part[F]: Partial[F] = new Partial[F]("part")
}

// ---------------------------------------------------------------- (C) ----

trait Box[T] { def show: String }

class FunBox[F, P, U](val raw: F, val tag: String) extends Box[F] {
  def show = "FunBox(" + tag + ")"
}

trait Shape[A, P] { def tag: String }
trait Exe[B, U] { def tag: String }

trait Compilable[T, C <: Box[T]] { def compiled(raw: T): C }

object Compilable {
  // `P` and `U` occur in the result only through the second parameter, which
  // the call site leaves undetermined; `sh` and `ex` are what say what they
  // are.
  implicit def fn1[A, B, P, U](implicit sh: Shape[A, P],
                               ex: Exe[B, U]): Compilable[A => B, FunBox[A => B, P, U]] =
    new Compilable[A => B, FunBox[A => B, P, U]] {
      def compiled(raw: A => B) = new FunBox[A => B, P, U](raw, sh.tag + "/" + ex.tag)
    }
}

object Compiled {
  def apply[V, C <: Box[V]](raw: V)(implicit c: Compilable[V, C]): C = c.compiled(raw)
}

object Main {
  implicit val shape: Shape[Int, String] = new Shape[Int, String] { def tag = "sh" }
  implicit val exe: Exe[Long, Double] = new Exe[Long, Double] { def tag = "ex" }

  def main(args: Array[String]): Unit = {
    val q = new SeqQuery[String, Int]("q1", "e")
    println(Wild.asRep(q).label)
    println(Wild.asRepAt(q).label)
    println(Wild.queryExec[String, Int, Seq].name)

    println(TQ.apply[String](i => "v" + i).head)
    println(TQ[String](i => "w" + i).head)
    println(TQ.apply[String].head)
    println(Fac.part[Int]("x", n = 2))

    val fb: FunBox[Int => Long, String, Double] = Compiled { (i: Int) => i.toLong }
    println(fb.show)
    println(fb.raw(3L.toInt))
  }
}
