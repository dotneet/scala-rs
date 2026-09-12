// `private[p]` is not `ACC_PRIVATE`. nsc erases the qualifier to public (`javap`:
// `private[data] def unwrap` in `cats.data.NonEmptyChainImpl` comes out
// `public <A> Chain<A> unwrap(Object)`), because every class in `p` may read it
// and `ACC_PRIVATE` helps with none of them -- not even one in the same package,
// since only the class itself may read a private member. Emitting it private made
// `cats.data.NonEmptyChainOps.toChain` throw `IllegalAccessError` on its first
// call.
package outer {
  package data {
    object Impl {
      private[data] type Base
      private[data] trait Tag extends Any
      type Typ[A] <: Base with Tag
      private[data] def create[A](s: List[A]): Typ[A] = s.asInstanceOf[Typ[A]]
      private[data] def unwrap[A](s: Typ[A]): List[A] = s.asInstanceOf[List[A]]
      private[outer] def shallow(i: Int): Int = i + 1
      def of[A](xs: List[A]): Typ[A] = create(xs)
    }
    class Ops[A](private val value: Impl.Typ[A]) {
      import Impl.{create, unwrap}
      final def toList: List[A] = unwrap(value)
      final def prepend(a: A): Impl.Typ[A] = create(a :: toList)
    }
  }
  class User {
    def go: Int = data.Impl.shallow(41)
  }
}

object Main {
  def main(args: Array[String]): Unit = {
    val o = new outer.data.Ops[Int](outer.data.Impl.of(List(1, 2)))
    println(o.toList)
    println(new outer.data.Ops[Int](o.prepend(0)).toList)
    println(new outer.User().go)
  }
}
