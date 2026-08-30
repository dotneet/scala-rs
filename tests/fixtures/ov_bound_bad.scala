// Rule 8, the bounds half: the override's type parameter must accept at least
// what the overridden one's does. `[A]` may override `[A <: AnyRef]`; the
// reverse would refuse arguments the base accepts.
//
// scalac 2.13.16: "incompatible type in overriding".
object Main {
  class B { def f[A](x: A): A = x }
  class D extends B { override def f[A <: AnyRef](x: A): A = x }
  def main(args: Array[String]): Unit = println(new D().f("s"))
}
