// Three shapes in which a type parameter reaches inference only through an
// expected type that still carries an *undecided* position, written `_`.
//
// 1. `flatTraverse[G[_], A, B](fa)(f: A => G[F[B]])(implicit …)`: `B` occurs
//    nowhere but under the applied abstract constructor `G[F[B]]`, and the
//    third clause means the literal is typed against `A => _[T[_]]`. Reading
//    `X := T[_]` out of that wildcard for the *inner* call's own parameter
//    left the result `P.F[T[_]]`.
// 2. `tailRecM`'s `F.map(f(a0).value) { case … }`: the match decides `B`, and
//    the enclosing `F[Either[L, _]]` fixed it to a wildcard before the match
//    was ever typed.
// 3. `appForKle[F[_], A]: Apl2[({ type L[x] = Kle[F, A, x] })#L]`: `A` occurs
//    only inside the type lambda, whose body lives beside its symbol rather
//    than in the type, so the expected type said nothing and `A` was
//    minimised to `Nothing`.
//
// Everything prints a value, so an instantiation that merely compiles --
// `Nothing`, `Any`, `_` -- cannot pass.
//
// The last object is the converse, and the reason none of the three rules can
// be phrased on the *shape* of a wildcard: `Cache[(Seq[String], Class[_]),
// String]` is an existential the program wrote, and it is the only thing that
// can give `{ case (sq, cs) => … }` its pattern types.

trait FunK[F[_], G[_]] { def apply[A](fa: F[A]): G[A] }

trait Apl[G[_]] {
  def pure[A](a: A): G[A]
  def map2[A, B, C](ga: G[A], gb: G[B])(f: (A, B) => C): G[C]
}

trait FlatM[F[_]] { def flatten[A](ffa: F[F[A]]): F[A] }

trait Trav[F[_]] {
  def flatTraverse[G[_], A, B](fa: F[A])(f: A => G[F[B]])(implicit G: Apl[G], F: FlatM[F]): G[F[B]]
}

trait Par[M[_]] {
  type F[_]
  def parallel: FunK[M, F]
  def applicative: Apl[F]
  // Only so the driver can print without reducing `P.F` at an instantiated
  // prefix, which is a different question (see `agent/absproj`).
  def toList[A](fa: F[A]): List[A]
}

object ListTrav extends Trav[List] {
  def flatTraverse[G[_], A, B](fa: List[A])(f: A => G[List[B]])(implicit
    G: Apl[G],
    F: FlatM[List]
  ): G[List[B]] =
    fa.foldRight(G.pure(List.empty[B]))((a, acc) => G.map2(f(a), acc)((x, y) => x ++ y))
}

object ListApl extends Apl[List] {
  def pure[A](a: A): List[A] = List(a)
  def map2[A, B, C](ga: List[A], gb: List[B])(f: (A, B) => C): List[C] =
    for (a <- ga; b <- gb) yield f(a, b)
}

object ListFlatM extends FlatM[List] {
  def flatten[A](ffa: List[List[A]]): List[A] = ffa.flatten
}

object OptionToList extends FunK[Option, List] {
  def apply[A](fa: Option[A]): List[A] = fa.toList
}

object OptionPar extends Par[Option] {
  type F[x] = List[x]
  def parallel: FunK[Option, List] = OptionToList
  def applicative: Apl[List] = ListApl
  def toList[A](fa: List[A]): List[A] = fa
}

// ------------------------------------------------------------------ shape 2

trait Mon[F[_]] {
  def map[A, B](fa: F[A])(f: A => B): F[B]
  def tailRecM[A, B](a: A)(f: A => F[Either[A, B]]): F[B]
}

final case class ET[F[_], L, A](value: F[Either[L, A]])

object OptionMon extends Mon[Option] {
  def map[A, B](fa: Option[A])(f: A => B): Option[B] = fa.map(f)
  def tailRecM[A, B](a: A)(f: A => Option[Either[A, B]]): Option[B] = {
    var cur: A = a
    var out: Option[B] = None
    var go = true
    while (go) {
      f(cur) match {
        case None               => go = false
        case Some(Left(next))   => cur = next
        case Some(Right(b))     => out = Some(b); go = false
      }
    }
    out
  }
}

trait ETMonad[F[_], L] {
  implicit val F: Mon[F]
  def tailRecM[A, B](a: A)(f: A => ET[F, L, Either[A, B]]): ET[F, L, B] =
    ET(
      F.tailRecM(a)(a0 =>
        F.map(f(a0).value) {
          case Left(l)         => Right(Left(l))
          case Right(Left(a1)) => Left(a1)
          case Right(Right(b)) => Right(Right(b))
        }
      )
    )
}

object OptionETMonad extends ETMonad[Option, String] {
  implicit val F: Mon[Option] = OptionMon
}

// ------------------------------------------------------------------ shape 3

trait Apl2[G[_]] { def label: String }

trait Show[A] { def name: String }

final case class Kle[F[_], A, B](run: A => F[B])

object Kle {
  def appForKle[F[_], A](implicit F: Apl2[F], A: Show[A]): Apl2[({ type L[x] = Kle[F, A, x] })#L] =
    new Apl2[({ type L[x] = Kle[F, A, x] })#L] {
      def label: String = F.label + "@" + A.name
    }
}

object Instances {
  implicit object listApl2 extends Apl2[List] { def label: String = "List" }
  implicit object showInt extends Show[Int] { def name: String = "Int" }
}

// -------------------------------------------------------------- the converse

// A wildcard the *program* wrote is a type like any other, and where nothing
// else has an opinion it is the only thing that can give a `{ case … }` its
// pattern types. `scala/scala`'s `pos/t12899` reduced: the rules above must
// not touch this one.

trait Cache[K, V] { def load(k: K): V }

object Kafi {
  def build[K, V](): Cache[K, V] = null
  def build[K, V](c: Cache[K, V]): Cache[K, V] = c

  def mk(sq: Seq[String], cs: Class[_]): String = sq.mkString(",") + ":" + cs.getSimpleName

  val c1: Cache[(Seq[String], Class[_]), String] = build {
    case (sq, cs) => mk(sq, cs)
  }
}

// -------------------------------------------------------------------- driver

object Main {
  // The defect's own shape: `B` reachable only under `G[T[B]]`, third clause.
  def parFlatTraverse[T[_], M[_], A, B](
    tv: Trav[T],
    ta: T[A],
    f: A => M[T[B]],
    fm: FlatM[T]
  )(P: Par[M]): List[T[B]] = {
    val gtb: P.F[T[B]] = tv.flatTraverse(ta)(a => P.parallel(f(a)))(P.applicative, fm)
    P.toList(gtb)
  }

  def main(args: Array[String]): Unit = {
    val f: Int => Option[List[String]] = i => if (i > 0) Some(List("a" * i, "b" * i)) else None
    val out: List[List[String]] =
      parFlatTraverse[List, Option, Int, String](ListTrav, List(1, 2), f, ListFlatM)(OptionPar)
    // `mkString` on the element only type-checks when `B` really is `String`.
    println(out.map(_.mkString("+")).mkString(" | "))

    val stepped: ET[Option, String, Int] =
      OptionETMonad.tailRecM[Int, Int](0) { i =>
        val v: Option[Either[String, Either[Int, Int]]] =
          if (i < 3) Some(Right(Left(i + 1))) else Some(Right(Right(i * 10)))
        ET(v)
      }
    // `Right(v)` binds an `Int` only when `B` really is `Int`.
    println(stepped.value match {
      case Some(Right(v)) => "step=" + (v + 1)
      case other          => "step?" + other
    })

    import Instances._
    val a: Apl2[({ type L[x] = Kle[List, Int, x] })#L] = Kle.appForKle
    println(a.label)

    println(Kafi.c1.load((Seq("x", "y"), classOf[String])))
  }
}
