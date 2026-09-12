// nsc's `VarianceValidator` does not check an object-private member, so none
// of these declarations is an error -- `final class LazyList[+A]
// private(private[this] var lazyState: () => LazyList.State[A])`,
// `private[this] def iterateUntilEmpty` in `IterableOps[+A, …]`,
// `private[this] implicit def ccClassTag[X]: ClassTag[CC[X]]` in
// `ClassTagIterableFactory[+CC[_]]`.
object Main {
  final class A[+T](private[this] var st: () => T) {
    private[this] def take(t: T): Int = 0
    private[this] val f: T => Int = _ => 0
    private[this] implicit def arr[X]: Array[T] = null
    protected[this] def alsoFine(t: T): Int = 0
    def get: T = st()
  }
  // An object-private constructor's parameters are object-private too.
  final class B[+T] private (x: T => Int) { def n: Int = 1 }
  object B { def of[T](x: T => Int): B[T] = new B(x) }
  final class C[-F, T] private[this] (arg: F) { def n: Int = 2 }

  def main(args: Array[String]): Unit = {
    println(new A[Int](() => 3).get)
    println(B.of[Int](_ => 0).n)
  }
}
