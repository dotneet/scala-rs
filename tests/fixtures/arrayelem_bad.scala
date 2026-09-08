// The element `new Array(n)` reads off the expected type is still an element
// an array has to be *made* of, so an abstract one with no `ClassTag` in scope
// is rejected -- not silently built as an `Array[Nothing]` and stored into.
//
// Real scalac 2.13.16 reports exactly this, and only this:
//   arrayelem_bad.scala:15: error: cannot find class tag for element type K
//
// A `new Array(n)` with *nothing at all* to read from is not the negative
// case: scalac accepts that one and instantiates the element to `Nothing`
// (`ClassTag.Nothing.newArray`, which is an `Object[]`), so it is not here.
object Main {
  final class Cell[A](val slots: Array[A])

  def mk[K]: Cell[K] = new Cell[K](new Array(0))

  def main(args: Array[String]): Unit = {
    println(mk[String].slots.length)
  }
}
