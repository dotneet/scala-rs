// `agent/kindvar`, `-Ykind-projector` only (scalac needs the plugin, so
// there is no scalac-agreement test; each shape's explicit spelling is in
// `kvar_kind_bad.scala` / `kvar_lambda_bad.scala`). The kind check reads
// kind-projector's variance marks: `Either[String, +*]` is an `F[+_]`,
// `Either[String, *]` is not, and `λ[`-a` => Cov[a]]` puts `a` in a covariant
// position. Lines marked `// error` are the ones rejected.
trait Cov[+A]
class Foo[F[+_]]
class Bar[F[-_]]

object KpBad {
  def m[F[+_]](x: F[Int]): F[Any] = x
  def n[F[-_]](x: F[Int]): F[Nothing] = x
  def foo[M[_]]: Int = 1

  new Foo[Either[String, +*]]
  new Bar[-* => Int]
  m[Either[String, +*]](Right(1))
  n[-* => Int]((x: Int) => x)
  foo[λ[`+a` => Cov[a]]]
  foo[λ[a => Cov[a]]]

  new Foo[Either[String, *]] // error
  new Bar[* => Int] // error
  m[Either[String, *]](Right(1)) // error
  n[* => Int]((x: Int) => x) // error
  foo[λ[`-a` => Cov[a]]] // error
}
