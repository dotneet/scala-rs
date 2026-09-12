// scala/scala `test/files/neg/t7872b.scala`: type lambdas whose parameter
// occurs at the wrong variance. The exact first message is pinned in
// `crates/cli/tests/kvar.rs` against scalac's.
object coinv {
  def up[F[+_]](fa: F[String]): F[Object] = fa
  def down[F[-_]](fa: F[Object]): F[String] = fa

  up(List("hi"))

  // should not compile; `l` is unsound
  def oops1 = down[({type l[-a] = List[a]})#l](List('whatever: Object)).head + "oops"

  type Stringer[-A] = A => String
  down[Stringer](_.toString)

  // should not compile; `l` is unsound
  def oops2 = up[({type l[+a] = Stringer[a]})#l]("printed: " + _)
}
