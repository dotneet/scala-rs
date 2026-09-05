// Three roots from typelevel/cats' tail (`agent/catstail3`). Each block is
// the smallest shape that reproduces one of them; scalac 2.13.16 accepts the
// whole file.

// ---------------------------------------------------------------- root 1
// `cats.Parallel`: `sequential` and `parallel` are *values* of a natural
// transformation type, so `P.sequential(fta)` auto-applies the parameterless
// def and then applies `FunctionK.apply[A](fa: F[A]): G[A]`. That inserted
// `apply` never had its own type parameters solved, so the call's type was
// the declaration's `M[A]`, with `A` still `apply`'s own parameter.
trait FnK[F[_], G[_]] {
  def apply[A](fa: F[A]): G[A]
}
trait NEP[M[_]] {
  type F[_]
  def sequential: FnK[F, M]
  def parallel: FnK[M, F]
}

object VecList extends NEP[List] {
  type F[x] = Vector[x]
  def sequential: FnK[Vector, List] = new FnK[Vector, List] {
    def apply[A](fa: Vector[A]): List[A] = fa.toList
  }
  def parallel: FnK[List, Vector] = new FnK[List, Vector] {
    def apply[A](fa: List[A]): Vector[A] = fa.toVector
  }
}

object Par {
  // The shape of `Parallel.parSequence`: the element type is `T[A]`, not `A`,
  // and only the argument says so.
  def roundTrip[M[_], A](P: NEP[M])(ma: M[List[A]]): M[List[A]] =
    P.sequential(P.parallel(ma))
}

// ---------------------------------------------------------------- root 2
// A parameterless collection member is declared to return `C` (`tail`,
// `init`) or `CC[B]` (`zipWithIndex`, `flatten`). Member completion installs
// an inherited declaration on the class it was *asked* about, with that
// class's `C` substituted in -- so once `bySeq` below has put
// `IterableOps.tail` on `immutable.Seq` as `Seq[A]`, `byVector` found it by
// inheritance and was a `Seq[A]` too. **The order of these two matters**: with
// `byVector` first the file compiled.
object Coll {
  def bySeq(xs: Seq[Int]): Seq[Int] = xs.tail
  def byVector(xs: Vector[Int]): Vector[Int] = xs.tail
  def initOf(xs: Vector[Int]): Vector[Int] = xs.init
  def indexed(xs: Vector[Int]): Vector[(Int, Int)] = xs.zipWithIndex
  def flat(xs: LazyList[Option[Int]]): LazyList[Int] = xs.flatten
}

// ---------------------------------------------------------------- root 3
// cats writes the same method at every level of its type-class tower with no
// `override` on any of them -- `Functor.compose[G[_]: Functor]`, then
// `Apply.compose[G[_]: Apply]`, ... -- because a different implicit parameter
// makes each one an *overload*. The override check refused to compare two
// parameter types that mention a type parameter and assumed they were the
// same, so all nine were "`override` modifier required".
trait Inv[F[_]] {
  def name: String
}
trait Fun[F[_]] extends Inv[F] {
  def compose[G[_]](implicit ev: Fun[G]): String = "Fun+" + ev.name
}
trait Ap[F[_]] extends Fun[F] {
  def compose[G[_]](implicit ev: Ap[G]): String = "Ap+" + ev.name
}

object AListInv extends Inv[List] { def name = "inv" }
object AListFun extends Fun[List] { def name = "fun" }
object AListAp extends Ap[List] { def name = "ap" }

object Main {
  def main(args: Array[String]): Unit = {
    println(Par.roundTrip(VecList)(List(List(1, 2), List(3))))
    println(Coll.bySeq(Seq(1, 2, 3)))
    println(Coll.byVector(Vector(1, 2, 3)))
    println(Coll.initOf(Vector(1, 2, 3)))
    println(Coll.indexed(Vector(7, 8)))
    println(Coll.flat(LazyList(Some(1), None, Some(3))).toList)
    println(AListAp.compose[List](AListAp))
    println(AListFun.compose[List](AListFun))
  }
}
