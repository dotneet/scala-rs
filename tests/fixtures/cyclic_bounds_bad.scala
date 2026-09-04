// A type parameter whose upper bound is the parameter itself. Real scalac
// 2.13.16 answers two diagnostics here:
//
//   error: illegal cyclic reference involving type A     (at the use, `x: A`)
//   error: cyclic aliasing or subtyping involving type A (at the definition)
//
// scala-rs reports the second. Before cycle detection existed this program
// took the whole compiler down with a stack overflow and no diagnostic at
// all -- the shape of scala/scala's `neg/t2918` and `neg/t5093`.
object Main {
  def f[A <: A](x: A): A = x
  def main(args: Array[String]): Unit = println(f(1))
}
