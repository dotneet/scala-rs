// A higher-kinded type member read through an instance prefix.
//
// `Par` is cats' `NonEmptyParallel` cut down to what the defect needs: an
// abstract `type F[_]` that every instance refines, and methods written in
// terms of `P.F`. Reading `P.F` on a particular `P` has to give that
// instance's refinement; producing the declaration's own unrefined member
// instead made the two sides print almost identically and compare unequal.
//
// The three shapes that matter, in order:
//
//   * `boxPar.F` is `Box`, so `boxPar.par(...)` is usable as a `Box`;
//   * `wrap`'s result names its own implicit parameter's `F` from inside a
//     type lambda (`Par.Aux[…, ({ type R[x] = Cell[P.F[x]] })#R]`), which is
//     cats' `Parallel.Aux[EitherT[M, E, *], Nested[P.F, Validated[E, *], *]]`;
//   * `wrapAgain` forwards to `wrap[M]` and must line the callee's `P` up with
//     its own -- the implicit argument is inserted by the compiler, so the
//     dependent-method-type substitution has to run on a clause nobody wrote.
//
// Everything here is `--no-scala-library`-clean and prints.

case class Box[A](a: A) {
  def label: String = "Box(" + a.toString + ")"
}
case class Cell[A](a: A) {
  def label: String = "Cell(" + a.toString + ")"
}
case class One[A](a: A)

trait Par[M[_]] {
  type F[_]
  def par[A](ma: M[A]): F[A]
  def seq[A](fa: F[A]): M[A]
}

object Par {
  type Aux[M[_], F0[_]] = Par[M] { type F[x] = F0[x] }
  def apply[M[_]](implicit P: Par[M]): Par[M] = P
}

object Instances {
  implicit val onePar: Par.Aux[One, Box] = new Par[One] {
    type F[x] = Box[x]
    def par[A](ma: One[A]): Box[A] = Box(ma.a)
    def seq[A](fa: Box[A]): One[A] = One(fa.a)
  }

  // The result's second argument is a type lambda whose body names `P.F`.
  def wrap[M[_]](implicit
    P: Par[M]
  ): Par.Aux[({ type L[x] = Cell[M[x]] })#L, ({ type R[x] = Cell[P.F[x]] })#R] =
    new Par[({ type L[x] = Cell[M[x]] })#L] {
      type F[x] = Cell[P.F[x]]
      def par[A](ma: Cell[M[A]]): Cell[P.F[A]] = Cell(P.par(ma.a))
      def seq[A](fa: Cell[P.F[A]]): Cell[M[A]] = Cell(P.seq(fa.a))
    }

  // A forwarder with the same signature. Its `P` and `wrap`'s `P` are two
  // different symbols, and the inserted implicit argument is what says they
  // denote the same value here.
  def wrapAgain[M[_]](implicit
    P: Par[M]
  ): Par.Aux[({ type L[x] = Cell[M[x]] })#L, ({ type R[x] = Cell[P.F[x]] })#R] =
    wrap[M]
}

object Main {
  def main(args: Array[String]): Unit = {
    import Instances._

    // `onePar.F` is `Box`, not the declaration's opaque `F`.
    val b: Box[Int] = onePar.par(One(1))
    println(b.label)
    val back: One[Int] = onePar.seq(Box(2))
    println(back.a)

    // Read through a path bound to a `val`, and through a parameter.
    val p: Par.Aux[One, Box] = onePar
    val viaVal: p.F[Int] = p.par(One(3))
    println(viaVal.label)
    println(readBack(onePar)(Box(4)).a)

    // The type lambda case, both through `wrap` and through the forwarder.
    val w = wrap[One]
    val cw: Cell[Box[Int]] = w.par(Cell(One(5)))
    println(cw.a.label)
    val w2 = wrapAgain[One]
    val cw2: Cell[Box[Int]] = w2.par(Cell(One(6)))
    println(cw2.a.label)
    println(w2.seq(Cell(Box(7))).a.a)
  }

  // `q.F` in a later parameter clause and in the body is one and the same
  // member, and at the call site it is the *argument's* member.
  def readBack[M[_]](q: Par[M])(fa: q.F[Int]): M[Int] = q.seq(fa)
}
