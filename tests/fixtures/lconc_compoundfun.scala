// A *compound* value applied as a function, and a trait whose declared parent
// is a function type.
//
//   * `val b: ForkJoinPool.ManagedBlocker with (() => T)` in
//     `scala/concurrent/impl/ExecutionContextImpl.scala` is applied as `b()`.
//     The `apply` comes from the function half of the compound, which this
//     representation keeps structural rather than as a `FunctionN` class.
//   * `trait PartialFunction[-A, +B] extends (A => B)` inherits `Function1`'s
//     `apply`, which `applyOrElse`'s body calls unqualified.
object Main {
  trait Marker { def block(): Boolean }

  def blocked[T](t: T): String = {
    val b: Marker with (() => T) = new Marker with (() => T) {
      def block(): Boolean = true
      def apply(): T = t
    }
    // Both spellings: the implied `.apply` and the written one.
    b().toString + b.apply().toString + b.block().toString
  }

  // The parent order reversed, and the function parent carrying arguments.
  def mapped(s: String): String = {
    val f: ((String => Int) with Marker) = new Marker with (String => Int) {
      def block(): Boolean = false
      def apply(x: String): Int = x.length
    }
    f(s).toString + f.block().toString
  }

  trait MyPF[-A, +B] extends (A => B) {
    def isDefinedAt(x: A): Boolean
    def orZero[A1 <: A, B1 >: B](x: A1, default: A1 => B1): B1 =
      if (isDefinedAt(x)) apply(x) else default(x)
  }

  def main(args: Array[String]): Unit = {
    println(blocked("q"))
    println(mapped("abcd"))
    val p: MyPF[Int, String] = new MyPF[Int, String] {
      def isDefinedAt(x: Int) = x > 0
      def apply(x: Int) = "n" + x
    }
    println(p.orZero(1, (_: Int) => "z"))
    println(p.orZero(-1, (_: Int) => "z"))
    // The trait really is a `Function1`, so it composes like one.
    println(p.andThen((s: String) => s + "!")(2))
  }
}
