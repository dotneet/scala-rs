// agent/libzero2: what the inference and conformance fixes must still refuse.
// Real scalac 2.13.16 rejects every case below.
object Main {
  // 1. Reading a method's type argument off a *function* expected type does not
  //    make an unrelated function type conform.
  trait Sub[-A, +B] extends (A => B)
  trait Eq[A, B] extends Sub[A, B]
  object Eq { def refl[A]: Eq[A, A] = new Eq[A, A] { def apply(x: A): A = x } }
  def bad1: Int => String = Eq.refl

  // 2. A constructor self-call's formals are an expected type, not a licence:
  //    the argument still has to conform.
  trait Hashing[T]
  class Default[T] extends Hashing[T]
  class Hashed[K](h: Hashing[K]) {
    def this(x: Int) = this(new Default[String])
  }

  // 3. Two alternatives that really are two alternatives: neither accepts the
  //    other's argument.
  trait Growable[A] {
    final def ++=(xs: IterableOnce[A]): this.type = this
  }
  class Chars extends Growable[Char] { def ++=(s: String): this.type = this }
  def bad3(c: Chars): Chars = c ++= List(1, 2, 3)

  // 4. A compound on the right of `<:` needs *every* component.
  trait MyMap[K, +V]
  trait Other[K]
  trait BF[-From, -A, +C]
  def bad4[CC[X, Y] <: MyMap[X, Y], K, V]: BF[Any, Int, CC[K, V] with Other[K]] =
    new BF[Any, Int, CC[K, V]] {}

  // 5. A `Unit` SAM result discards a value; a non-`Unit` one does not.
  def bad5: java.util.function.BiFunction[String, String, Int] = (a, b) => a + b
}
