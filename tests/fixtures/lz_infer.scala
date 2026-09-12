// Candidate fixture: inference roots.
object Infer {
  // 1. A *value* of function type is not applicable to a `PartialFunction`
  //    formal, so the overload that takes one is not even a candidate.
  val pf: PartialFunction[Int, String] = { case 1 => "one" }
  val k: String => Int = _.length
  val chained: PartialFunction[Int, Int] = pf andThen k

  // 2. A `Nothing` the arguments inferred is instantiated where the parameter
  //    is covariant in the result, expected type or not.
  def mk[T](f: Throwable => T): PartialFunction[Throwable, T] = { case t => f(t) }
  val nothingCatcher: PartialFunction[Throwable, Nothing] = mk(throw _)
  def anyCatcher[T]: PartialFunction[Throwable, T] = mk(throw _)

  // 3. `[B, B1 >: B]`: the bound that names another parameter of the same
  //    method is applied once that one is solved.
  class Cov[A, +B](val b: B)
  def upd[A, B, B1 >: B](t: Cov[A, B], v: B1): Cov[A, B1] = new Cov[A, B1](v)
  def widened[A](t: Cov[A, Any]): Cov[A, Any] = upd(t, null)

  // 4. `Null` / `Nothing` in an *invariant* position of the expected type is
  //    the answer.
  class Inv[A, B](val tag: String)
  object Inv { def empty[A, B]: Inv[A, B] = new Inv[A, B]("empty") }
  def nullTree[E]: Inv[E, Null] = Inv.empty

  // 5. A contravariant-only occurrence bounds the parameter from above, so it
  //    loses to the position that pinned it -- and is still the answer when
  //    nothing else contributes.
  def aoe[A <: AnyVal, B](x: A, default: A => B): B = default(x)
  def fallbackChar(c: Char, f: Any => Any): Any = aoe(c, f)
  def sink[T](f: T => Unit): Unit = ()
  def useSink(): Unit = sink((_: String) => ())

  // 6. An `implicit def` member of the enclosing class answers a search whose
  //    method has *other* type parameters the clause does not mention.
  trait Ord[A] { def name: String }
  def needs[K: Ord, V]: String = implicitly[Ord[K]].name
  class WithDefault[K, V](o: Ord[K]) {
    implicit def ordering: Ord[K] = o
    def describe: String = needs
  }
}

object Main {
  def main(args: Array[String]): Unit = {
    println(Infer.chained(1))
    println(Infer.nothingCatcher.isDefinedAt(new RuntimeException("x")))
    println(Infer.widened(new Infer.Cov[String, Any]("a")).b)
    println(Infer.nullTree[Int].tag)
    println(Infer.fallbackChar('c', x => x))
    Infer.useSink()
    println(new Infer.WithDefault[Int, String](new Infer.Ord[Int] { def name = "int-ord" }).describe)
  }
}
