import scala.collection.Factory
object PrototypeMerge {
  trait HList
  class HNil extends HList
  class Cons[H, T <: HList] extends HList
  trait ToTrav[L, M[_]] { type Lub }
  object ToTrav {
    type Aux[L, M[_], A] = ToTrav[L, M] { type Lub = A }
    trait Lub[-A, -B, C]
    val broad: Lub[AnyRef, AnyRef, AnyRef] = new Lub[AnyRef, AnyRef, AnyRef] {}
    def single[T, M[_], A](ev: T <:< A, factory: Factory[A, M[A]]): Aux[Cons[T, HNil], M, A] = null
    def cons[H, T <: HList, B, A, M[_]](tail: Aux[T, M, B], ev: Lub[H, B, A], factory: Factory[A, M[A]]): Aux[Cons[H, T], M, A] = null
  }
  class Alpha
  class Beta
  val tail: ToTrav.Aux[Cons[Beta, HNil], List, AnyRef] =
    ToTrav.single(scala.<:<.refl, List.iterableFactory)
  val result: ToTrav.Aux[Cons[Alpha, Cons[Beta, HNil]], List, AnyRef] =
    ToTrav.cons(tail, ToTrav.broad, List.iterableFactory)
}
object Main extends App { println("ok") }
