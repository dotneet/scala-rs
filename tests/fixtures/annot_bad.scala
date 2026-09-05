// An annotation this subset does not implement is a diagnostic, not something
// silently dropped. `@strictfp` is the remaining one: nsc emits ACC_STRICT for
// it, and we would emit a method that does not have it.
//
// This fixture used to write `@specialized`, which is now accepted and
// recorded (docs/specialization.md) -- and which real scalac accepts on a
// method with no type parameters too, silently doing nothing.
object Main {
  @strictfp
  def f(): Int = 1
  def main(args: Array[String]): Unit = {
    println(f())
  }
}
