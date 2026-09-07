// Partial unification: a higher-kinded type variable solved against a type
// applied to more arguments than the variable takes. nsc (`TypeVar.unifyFull`,
// the rule from scala/bug#2712) reads the constructor as curried: the leftmost
// surplus arguments are captured as constants and only the rightmost ones are
// abstracted, so `Either[String, Int]` against `F[A]` is `F := Either[String, *]`
// and `A := Int` -- never `[x]Either[x, Int]`. Every line below prints the
// name of the type an implicit was found for, so an instantiation that merely
// compiles cannot pass: the printed name is the solution.
//
// Expected output is what real scalac 2.13.16 prints for this source.
object Main {
  trait Name[A] { def name: String }
  implicit val nameInt: Name[Int] = new Name[Int] { def name = "Int" }
  implicit val nameString: Name[String] = new Name[String] { def name = "String" }
  implicit val nameBoolean: Name[Boolean] = new Name[Boolean] { def name = "Boolean" }
  implicit val nameLong: Name[Long] = new Name[Long] { def name = "Long" }

  // A one-parameter variable against two-, three- and four-parameter types.
  def pick[F[_], A](fa: F[A])(implicit n: Name[A]): String = n.name
  def two[F[_, _], A, B](fab: F[A, B])(implicit a: Name[A], b: Name[B]): String = a.name + "," + b.name
  class Tri[X, Y, Z](val x: X, val y: Y, val z: Z)
  class Quad[W, X, Y, Z]
  type Pair[A] = Either[A, A]

  // A state monad in the shape of cats' `IndexedStateT`: four parameters, the
  // last of which is the one `traverse`'s `G[_]` has to abstract.
  class IxSt[F[_], SA, SB, A](val run: SA => F[(SB, A)])
  type Id[A] = A
  type St[S, A] = IxSt[Id, S, S, A]
  object St {
    def apply[S, A](f: S => (S, A)): St[S, A] = new IxSt[Id, S, S, A](f)
  }

  trait Ap[G[_]] {
    def pure[A](a: A): G[A]
    def map2[A, B, C](ga: G[A], gb: G[B])(f: (A, B) => C): G[C]
  }
  implicit def apSt[S]: Ap[({ type L[x] = St[S, x] })#L] =
    new Ap[({ type L[x] = St[S, x] })#L] {
      def pure[A](a: A): St[S, A] = St(s => (s, a))
      def map2[A, B, C](ga: St[S, A], gb: St[S, B])(f: (A, B) => C): St[S, C] =
        St { s =>
          val (s1, a) = ga.run(s)
          val (s2, b) = gb.run(s1)
          (s2, f(a, b))
        }
    }
  def traverse[G[_], A, B](xs: List[A])(f: A => G[B])(implicit G: Ap[G]): G[List[B]] =
    xs.foldRight(G.pure(List.empty[B]))((a, acc) => G.map2(f(a), acc)((b, bs) => b :: bs))

  // cats' `Traverse.mapAccumulate`, in its own spelling: `s` has no written
  // type and is read off `f`, and `G` is solved to `St[S, *]` from the
  // literal's result -- which is what finds `apSt[S]`.
  def mapAccumulate[S, A, B](init: S, xs: List[A])(f: (S, A) => (S, B)): (S, List[B]) =
    traverse(xs)(a => St(s => f(s, a))).run(init)
  // cats' `traverseWithIndexM`: the same with an annotated parameter.
  def withIndex[A, B](xs: List[A])(f: (A, Int) => B): (Int, List[B]) =
    traverse(xs)(a => St((s: Int) => (s + 1, f(a, s)))).run(0)

  // The expected-type direction. nsc captures the same way when the variable
  // sits in an *invariant* or *contravariant* position of the expected type;
  // in a covariant one it is minimised to `Nothing` instead, which is not
  // this fixture's business.
  class Inv[A]
  class Ctr[-A]
  def mkInv[G[_], A](a: A)(implicit n: Name[A]): Inv[G[A]] = {
    println("mkInv: " + n.name)
    new Inv[G[A]]
  }
  def mkCtr[G[_], A](a: A)(implicit n: Name[A]): Ctr[G[A]] = {
    println("mkCtr: " + n.name)
    new Ctr[G[A]]
  }

  def main(args: Array[String]): Unit = {
    val e: Either[String, Int] = Right(1)
    println("pick Either: " + pick(e))
    println("two Tri: " + two(new Tri[Int, String, Boolean](1, "s", true)))
    println("two Quad: " + two(new Quad[Long, Int, String, Boolean]))
    val p: Pair[Long] = Right(2L)
    println("pick alias: " + pick(p))
    val f1: Int => String = _.toString
    println("pick Function1: " + pick(f1))
    val t: (Int, Boolean) = (1, true)
    println("pick Tuple2: " + pick(t))
    println("two Tuple3: " + two((1L, "s", 2)))
    val st: St[Int, String] = St(s => (s + 1, "x"))
    println("pick IxSt: " + pick(st))
    val (n, ys) = mapAccumulate(10, List("a", "b", "c"))((s, a) => (s + 1, a + s))
    println("mapAccumulate: " + n + " " + ys)
    val (m, zs) = withIndex(List('x', 'y'))((c, i) => c.toString * (i + 1))
    println("withIndex: " + m + " " + zs)
    val ri: Inv[Either[String, Int]] = mkInv(7)
    val rc: Ctr[Either[String, Boolean]] = mkCtr(true)
  }
}
