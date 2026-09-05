// Compiled against `bt2_lib.scala`'s class files -- by real scalac 2.13.16
// over scala-rs's output, and by scala-rs over scalac's. Both runs must print
// what scalac-on-scalac prints. See `crates/cli/tests/traitclass.rs`.
import bt2lib._

class Sub extends Counter
// `def m` and `var dv` are *declarations* in `Decl`, so no `override` is owed;
// `val cv` is concrete there and is not restated.
class D extends Decl { def m = "d"; var dv = 3 }

object Main {
  def main(args: Array[String]): Unit = {
    val s = new Sub
    println(s.doubled)
    // Assignment to a `var` of a trait that arrived as a class file.
    s.count = 9
    println(s.tick)
    println(s.doubled)
    println(s.peek)
    val d = new D
    println(d.m + d.dv + d.cv)
    d.dv = 7
    println(d.dv)
  }
}
