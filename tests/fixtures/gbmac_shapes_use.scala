// Call sites for `gbmac_shapes_impl.scala`. Real scalac 2.13.16 and scala-rs
// must print the same (`crates/cli/tests/gbmac.rs`).
import gbmacs.Shapes

object Main {
  def main(args: Array[String]): Unit = {
    val f = Shapes.classify
    println(f(1))
    println(f("x"))
    println(f(""))
    println(f(List(1, 2, 3)))
    println(f(Map(1 -> 2)))
    println(f((7, "y")))
    println(f(3.5))
    println(Shapes.named(2))
    println(Shapes.sizeOf(List("a", "b", "c")))
  }
}
