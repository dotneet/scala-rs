// A nested polymorphic argument solved together with the outer call through
// nsc's lenient prototype: `strictOptimizedMap(iterableFactory.newBuilder, f)`
// reads `newBuilder`'s own `A` out of the `CC[B]` the declared result put
// into its expected type `Builder[_, CC[B]]` -- the only place it can be
// read from, `Builder` being contravariant in its element. This is the
// library's `StrictOptimized{Iterable,Map,Seq,SortedMap}Ops` shape with a
// user-defined (abstract, hence invariant) collection constructor.
import scala.collection.mutable
object Main {
  trait Fac[CC[_]] { def newBuilder[A]: mutable.Builder[A, CC[A]] }
  trait MFac[CC[_, _]] { def newBuilder[K, V]: mutable.Builder[(K, V), CC[K, V]] }
  trait Ops[A, CC[_]] {
    def iterableFactory: Fac[CC]
    def iterator: Iterator[A]
    def map[B](f: A => B): CC[B] = strictOptimizedMap(iterableFactory.newBuilder, f)
    def flatMap[B](f: A => IterableOnce[B]): CC[B] = strictOptimizedFlatMap(iterableFactory.newBuilder, f)
    def flatten[B](implicit ev: A => IterableOnce[B]): CC[B] = strictOptimizedFlatten(iterableFactory.newBuilder)
    def concat[B >: A](suffix: IterableOnce[B]): CC[B] = strictOptimizedConcat(suffix, iterableFactory.newBuilder)
    def collectPf[B](pf: PartialFunction[A, B]): CC[B] = strictOptimizedCollect(iterableFactory.newBuilder, pf)
    def strictOptimizedMap[B, C2](b: mutable.Builder[B, C2], f: A => B): C2 = {
      val it = iterator
      while (it.hasNext) b += f(it.next())
      b.result()
    }
    def strictOptimizedFlatMap[B, C2](b: mutable.Builder[B, C2], f: A => IterableOnce[B]): C2 = {
      val it = iterator
      while (it.hasNext) b ++= f(it.next())
      b.result()
    }
    def strictOptimizedFlatten[B, C2](b: mutable.Builder[B, C2])(implicit toIterableOnce: A => IterableOnce[B]): C2 = {
      val it = iterator
      while (it.hasNext) b ++= toIterableOnce(it.next())
      b.result()
    }
    def strictOptimizedConcat[B >: A, C2](that: IterableOnce[B], b: mutable.Builder[B, C2]): C2 = {
      b ++= iterator
      b ++= that
      b.result()
    }
    def strictOptimizedCollect[B, C2](b: mutable.Builder[B, C2], pf: PartialFunction[A, B]): C2 = {
      val it = iterator
      while (it.hasNext) { val x = it.next(); if (pf.isDefinedAt(x)) b += pf(x) }
      b.result()
    }
  }
  trait MOps[K, V, CC[_, _]] extends Ops[(K, V), Iterable] {
    def mapFactory: MFac[CC]
    def mapK[K2, V2](f: ((K, V)) => (K2, V2)): CC[K2, V2] = strictOptimizedMap(mapFactory.newBuilder, f)
    def concatK[V2 >: V](suffix: IterableOnce[(K, V2)]): CC[K, V2] = strictOptimizedConcat(suffix, mapFactory.newBuilder)
    def collectK[K2, V2](pf: PartialFunction[(K, V), (K2, V2)]): CC[K2, V2] = strictOptimizedCollect(mapFactory.newBuilder, pf)
  }
  class L[A](xs: List[A]) extends Ops[A, List] {
    def iterableFactory: Fac[List] = new Fac[List] { def newBuilder[X]: mutable.Builder[X, List[X]] = List.newBuilder[X] }
    def iterator: Iterator[A] = xs.iterator
  }
  class M[K, V](m: Map[K, V]) extends MOps[K, V, Map] {
    def iterableFactory: Fac[Iterable] = new Fac[Iterable] { def newBuilder[X]: mutable.Builder[X, Iterable[X]] = Iterable.newBuilder[X] }
    def mapFactory: MFac[Map] = new MFac[Map] { def newBuilder[X, Y]: mutable.Builder[(X, Y), Map[X, Y]] = Map.newBuilder[X, Y] }
    def iterator: Iterator[(K, V)] = m.iterator
  }
  // cats' VarianceConversions: the prototype's wildcard must not become a
  // solution of the inner call's receiver variable.
  trait Functor[F[_]] { def widen[A, B >: A](fa: F[A]): F[B] = fa.asInstanceOf[F[B]] }
  trait Bifunctor[F[_, _]] {
    def rightFunctor[X]: Functor[({ type L[a] = F[X, a] })#L] = new Functor[({ type L[a] = F[X, a] })#L] {}
    def leftWiden[A, B, AA >: A](fab: F[A, B]): F[AA, B] = fab.asInstanceOf[F[AA, B]]
  }
  def bf[F[_, _]](implicit F: Bifunctor[F]): Bifunctor[F] = F
  def auto[F[_, _]: Bifunctor, A, B >: A, C, D >: C](fac: F[A, C]): F[B, D] =
    bf[F].leftWiden(bf[F].rightFunctor.widen(fac))
  implicit val eb: Bifunctor[Either] = new Bifunctor[Either] {}

  def main(args: Array[String]): Unit = {
    val l = new L(List(1, 2, 3))
    println(l.map(_ + 1))
    println(l.map(_.toString + "!"))
    println(l.flatMap(x => List(x, x)))
    println(l.concat(List(4)))
    println(l.collectPf { case x if x > 1 => x * 10 })
    println(new L(List(List(1), List(2))).flatten)
    val m = new M(Map(1 -> "a", 2 -> "b"))
    println(m.mapK { case (k, v) => (v, k) })
    println(m.concatK(List(3 -> "c")))
    println(m.collectK { case (k, v) if k > 1 => (k, v + v) })
    val e: Either[AnyRef, Any] = auto[Either, String, AnyRef, Int, Any](Right(1))
    println(e)
  }
}
