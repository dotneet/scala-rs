// `Option.getOrElse[B >: A]` / `orElse[B >: A]`: the argument widens the
// result, it does not have to fit the receiver's element type.
object Main {
  class Base { override def toString = "Base" }
  class Sub extends Base { override def toString = "Sub" }

  def describe(b: Base): String = "got " + b.toString

  def main(args: Array[String]): Unit = {
    val some: Option[Sub] = Some(new Sub)
    val none: Option[Sub] = None
    val b: Base = new Base

    // Before `[B >: A]` these were `no matching overload`: a `Base` argument
    // handed to a `(=> Sub)Sub`.
    val w1: Base = some.getOrElse(b)
    val w2: Base = none.getOrElse(b)
    println(describe(w1))
    println(describe(w2))
    println(describe(none.orElse(Some(b)).get))
    println(describe(some.orElse(Some(b)).get))

    // A completely unrelated default: the result is the lub.
    val any: Any = none.getOrElse("fallback")
    println(any)

    // Nothing does not widen anything.
    val s: Sub = some.getOrElse(throw new RuntimeException("boom"))
    println(s)
  }
}
