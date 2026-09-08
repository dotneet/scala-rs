// `Array`'s one constructor takes the length, and the element coming from the
// expected type does not change that. This is scala/scala's own
// `test/files/neg/multi-array.scala` -- the multi-dimensional `new Array(n, n)`
// removed in 2.10 -- whose `.check` is
//
//   error: too many arguments (found 2, expected 1) for constructor Array: (_length: Int): Array[T]
//
// nsc runs the arity check *before* it instantiates the element, so it names
// `Array[T]` here and `Array[Int]` when the element is written out.
//
// The un-annotated form used to be rejected for a different reason -- no
// constructor matched, because the element was never inferred -- and the
// annotated `new Array[Int](10, 10)` was accepted outright and emitted an
// `invokespecial` of a constructor no array class has.
object Main {
  val a: Array[Int] = new Array(10, 10)
  val b: Array[Int] = new Array[Int](10, 10)
  val c: Array[Int] = new Array[Int]()
  val d: Array[Int] = new Array[Int]

  def main(args: Array[String]): Unit = println(a.length + b.length + c.length + d.length)
}
