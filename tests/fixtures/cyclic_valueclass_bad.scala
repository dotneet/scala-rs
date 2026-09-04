// scala/scala's `neg/t5878`: two value classes that wrap each other. A value
// class erases to what it wraps, so this pair has no erasure -- `erase_ty`
// unboxed `Foo` to `Bar` to `Foo` until the stack ran out. scalac 2.13.16
// rejects it instead: `value class may not wrap another user-defined value
// class`, once per class.
class Foo1(val x: Bar1) extends AnyVal
class Bar1(val x: Foo1) extends AnyVal
object Main {
  def main(args: Array[String]): Unit = ()
}
