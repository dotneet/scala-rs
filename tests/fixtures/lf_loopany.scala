// The same merge where the declared class *is* `java/lang/Object`.
//
// `var a: Any` holds a boxed `Integer` before the loop and a `String` inside
// it. Recording only the loop head's frame as `Object` is not enough: this
// assembler emits its frames in one forward pass, so the frames inside the
// condition and the body were already written with `java/lang/Integer` and
// disagreed with what the back edge merged. Declaring the slot `Object` at
// every store is what makes the single pass produce consistent frames.
object Main {
  def main(args: Array[String]): Unit = {
    var a: Any = 1
    var i = 0
    while (i < 2) {
      a = if (i == 0) "s" else 2
      i += 1
    }
    println(a)

    // An array local reassigned in the loop: the frame entry is a descriptor,
    // not an internal class name.
    var arr: Array[Int] = new Array[Int](2)
    var j = 0
    while (j < 3) { arr = new Array[Int](j); j += 1 }
    println(arr.length)

    // Primitive locals stay primitive; nothing to widen.
    var n = 0
    var acc = 0L
    while (n < 4) { acc += n; n += 1 }
    println(acc)

    // A local that is only ever `null` before the loop.
    var z: String = null
    var m = 0
    while (m < 2) { z = "z" + m; m += 1 }
    println(z)
  }
}
