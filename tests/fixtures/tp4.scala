// `extends` a Java generic superclass with a primitive constructor argument
// (`agent/javanest` found and located this; found live in the wild as
// `class A1 extends java.util.concurrent.atomic.AtomicReference[Int](1)`).
// The expression-position `new AtomicReference[Int](1)` already boxed the
// `1`; the `extends` clause's own constructor call did not, and the JVM
// verifier rejected the unboxed `int` where `Object` was wanted.
class Counter2 extends java.util.concurrent.atomic.AtomicReference[Int](1) {
  def bump(): Int = {
    val v = get() + 1
    set(v)
    v
  }
}

object Main {
  def main(args: Array[String]): Unit = {
    val c = new Counter2
    println(c.get())
    println(c.bump())
    println(c.get())
  }
}
