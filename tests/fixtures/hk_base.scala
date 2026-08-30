// An argument's *base type* is what pins the callee's type parameters.
// `object IntBox extends Box[Int]` has type `IntBox.type`, and `Box[Int]` is
// only reachable through its base types -- first order and higher-kinded
// alike, and through `this.type` / `p.type` paths too.
object Main {
  trait Box[A] { def get: A }
  object IntBox extends Box[Int] { def get = 7 }
  class StrBox extends Box[String] { def get = "s" }
  def unbox[A](b: Box[A]): A = b.get

  trait Ctor[F[_]] { def make(n: Int): F[Int] }
  class IdBox[A](val a: A)
  object IdCtor extends Ctor[IdBox] { def make(n: Int) = new IdBox(n) }
  def build[F[_]](c: Ctor[F]): F[Int] = c.make(3)

  trait Self[A] {
    def id(x: A): A = x
    def me: this.type = this
  }
  object SelfInt extends Self[Int]
  val sv: SelfInt.type = SelfInt
  def useSelf[A](s: Self[A], x: A): A = s.id(x)

  def main(args: Array[String]): Unit = {
    println(unbox(IntBox))
    println(unbox(new StrBox))
    println(build(IdCtor).a)
    println(useSelf(SelfInt, 5))
    println(useSelf(SelfInt.me, 6))
    println(useSelf(sv, 8))
  }
}
