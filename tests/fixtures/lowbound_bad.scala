// `f(42)` violates `[A <: Named]`; nsc reports the inferred type arguments.
trait Named {
  def name: String
}

object Main {
  def f[A <: Named](x: A): Int = 1

  def main(args: Array[String]): Unit = {
    println(f(42))
  }
}
