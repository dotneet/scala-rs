// What `-Ykind-projector` must *not* do. Compiled with the flag, this file
// reports the same four errors real scalac 2.13.16 reports with the plugin on
// its classpath.
//
// The first two are shapes the plugin's rewriter does not recognise: it passes
// them through untouched, so `λ` is then an ordinary type name and nsc says it
// is not in scope. A diagnostic of our own here would be one nsc does not
// have.
//
// The last two are lambdas that are perfectly well formed and simply do not
// match, so that comparing two lambdas by their bodies cannot degenerate into
// accepting any two of them.
object Main {
  trait Functor[F[_]] { def map[A, B](fa: F[A])(f: A => B): F[B] }
  final case class Box[A](value: A)
  final case class Pair[A, B](a: A, b: B)

  // Not a function type, so not a lambda.
  def b1(x: Functor[λ[Int]]): Int = 1

  // Two type arguments: `λ` takes one.
  def b2(x: Functor[λ[α => Box[α], β]]): Int = 2

  val boxF: Functor[Box] = new Functor[Box] {
    def map[A, B](fa: Box[A])(f: A => B): Box[B] = Box(f(fa.value))
  }
  val b3: Functor[Pair[String, *]] = boxF
  val b4: Functor[λ[α => Pair[α, α]]] = boxF
}
