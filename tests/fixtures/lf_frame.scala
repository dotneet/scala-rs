// The minimized case. `c` starts as `Some[Int]` and the loop body stores a
// `None$` into it, so the loop-head frame has to describe the slot as their
// common supertype. scalac writes the slot's *declared* erased type there --
// `class scala/Option`, the same type its LocalVariableTable entry has -- and
// so must we; `java/lang/Object` verifies as a frame but makes the
// `invokevirtual scala/Option.isDefined` that reads the slot fail.
object Main {
  def main(a: Array[String]): Unit = {
    var c: Option[Int] = Some(1)
    while (c.isDefined) { c = None }
    println("done")
  }
}
