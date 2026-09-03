// agent/tq negative cases. Real scalac 2.13.16 rejects every one of these.

trait BRep[T] { def label: String }
trait BQueryBase[T] extends BRep[T]
trait BQuery[+E, U, C[_]] extends BQueryBase[C[U]]

trait BExec[T, TU, EU]

object BExec extends BExec[BRep[Any], Any, Any] {
  def apply[T <: BRep[_], TU, EU] = this.asInstanceOf[BExec[T, TU, EU]]
}

class BTQ[E](val cons: Int => E)

object BTQ {
  def apply[E](cons: Int => E): BTQ[E] = new BTQ[E](cons)
  def apply[E]: BTQ[E] = new BTQ[E](_ => null.asInstanceOf[E])
}

trait BBox[T]
class BFunBox[F, P, U](val raw: F) extends BBox[F]
trait BShape[A, P]
trait BExe[B, U]
trait BCompilable[T, C <: BBox[T]] { def compiled(raw: T): C }

object BCompilable {
  implicit def fn1[A, B, P, U](implicit sh: BShape[A, P],
                               ex: BExe[B, U]): BCompilable[A => B, BFunBox[A => B, P, U]] =
    new BCompilable[A => B, BFunBox[A => B, P, U]] {
      def compiled(raw: A => B) = new BFunBox[A => B, P, U](raw)
    }
}

object BCompiled {
  def apply[V, C <: BBox[V]](raw: V)(implicit c: BCompilable[V, C]): C = c.compiled(raw)
}

object BadMain {
  // A wildcard argument does not make the *bound* vacuous: `String` is no
  // `BRep[_]` at all.
  val bad1 = BExec[String, Any, Any]

  // Keeping the overload set alive does not make a call that fits no
  // alternative legal.
  val bad2 = BTQ.apply[String]("not a function")

  // `P` has a witness, `U` has none: the candidate's own clause cannot be
  // completed, so there is no `BCompilable` at all.
  implicit val shape: BShape[Int, String] = new BShape[Int, String] {}
  val bad3: BFunBox[Int => Long, String, Double] = BCompiled { (i: Int) => i.toLong }
}
