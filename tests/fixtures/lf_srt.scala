// A generic `Array[T]` element access and `length` in `--no-scala-library` mode,
// with the run's OWN sources supplying `scala.runtime.ScalaRunTime`.
//
// nsc compiles `a(i)` / `a(i) = v` / `a.length` on an abstract element type into
// a call to that object, and the standard library supplies it out of
// `src/library/scala/runtime/ScalaRunTime.scala` -- 230 of the library's own
// sites are this shape. The gate used to ask whether the run links the released
// jar instead of whether it has the object, so those 230 were refused, and
// `length` silently emitted `arraylength` on an `Object`
// (`VerifyError: Bad type on operand stack in arraylength`).
//
// `tests/fixtures/arraygen_gate.scala` is the other direction: the same shape
// with no such object in the run still has to be a diagnostic. There is no
// scalac comparison for this file -- real scalac always has the jar's own
// `ScalaRunTime` and would see a duplicate.
package scala.runtime {
  object ScalaRunTime {
    def array_apply(xs: AnyRef, idx: Int): Any = xs.asInstanceOf[Array[AnyRef]](idx)
    def array_update(xs: AnyRef, idx: Int, value: Any): Unit =
      xs.asInstanceOf[Array[AnyRef]](idx) = value.asInstanceOf[AnyRef]
    def array_clone(xs: AnyRef): AnyRef = xs.asInstanceOf[Array[AnyRef]].clone()
    def array_length(xs: AnyRef): Int = xs.asInstanceOf[Array[AnyRef]].length
  }
}

object Main {
  def fill[T](a: Array[T], x: T): Unit = {
    var i = 0
    while (i < a.length) { a(i) = x; i += 1 }
  }
  def first[T](a: Array[T]): T = a(0)
  def copyOf[T](a: Array[T]): Array[T] = a.clone()
  def main(args: Array[String]): Unit = {
    val a = new Array[String](2)
    fill(a, "y")
    val b = copyOf(a)
    println(first(a) + a(1) + first(b))
  }
}
