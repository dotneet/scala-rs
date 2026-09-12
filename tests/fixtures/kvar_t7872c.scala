// scala/scala `test/files/neg/t7872c.scala`: an inferred `F := List` for a
// contravariant constructor parameter. The exact message is pinned in
// `crates/cli/tests/kvar.rs` against scalac's.
object coinv {
  def up[F[+_]](fa: F[String]): F[Object] = fa
  def down[F[-_]](fa: F[Object]): F[String] = fa

  up(List("hi"))
  // [error] type A is covariant, but type _ is declared contravariant
  down(List('whatever: Object))
}
