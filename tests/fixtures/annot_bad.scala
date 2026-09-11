// An annotation is a class the program has to be able to name, not a word
// the compiler matches on. `@strictfp` without `import scala.annotation.strictfp`
// is "not found: type strictfp" in scalac 2.13.16, and the backend, which
// sets ACC_STRICT for the real one, must not honour it.
//
// This fixture used to check that `@strictfp` was reported as an
// unimplemented annotation (it was the last one); before that it wrote
// `@specialized`, which is now accepted and recorded
// (docs/specialization.md).
object Main {
  @strictfp
  def f(): Int = 1
  def main(args: Array[String]): Unit = {
    println(f())
  }
}
