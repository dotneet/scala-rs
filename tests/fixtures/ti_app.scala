// Compiled against `ti_lib.scala`'s class files — by real scalac 2.13.16 over
// scala-rs's output, and by scala-rs over scalac's. Both runs must print what
// scalac-on-scalac prints. See `crates/cli/tests/traitclass.rs`.
import tilib._

class Sub extends Counter

object Main {
  def main(args: Array[String]): Unit = {
    val s = new Sub
    println(s.base)
    println(s.bump)
    println(s.bump)
    println(s.show)
    val c: Counter = s
    // Through a trait method rather than `c.count = 9`; the direct assignment
    // to a binary trait's `var` is covered by `bt2_app.scala`.
    println(c.tick)
    println(c.plain + c.count)
  }
}
