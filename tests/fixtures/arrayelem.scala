// `new Array(n)` written with no type argument takes its element type from
// the expected type, the way nsc solves the `Array` constructor's parameter
// against `pt`. Every array below is built that way, then *written into and
// read back*: getting the element wrong is an `ArrayStoreException` or a
// `ClassCastException` at run time, not a compile error, so the run is the
// test, and `getClass.getName` pins the JVM array class each one really got.
//
// No `ClassTag` here on purpose -- this fixture runs in the private-runtime
// mode as well, which has no `scala.reflect`. The `ClassTag` half of the
// mechanism is `arrayelem_tag.scala`, which is library-mode only.
object Main {
  final class Cell[A](val slots: Array[A], val tag: String)

  def take(a: Array[String]): String = a(0) + "/" + a.length

  def main(args: Array[String]): Unit = {
    // A `val` with an ascription: primitive element.
    val ints: Array[Int] = new Array(3)
    ints(0) = 7
    ints(2) = 9
    println("ints " + ints(0) + " " + ints(2) + " " + ints.length)

    // A `val` with an ascription: reference element.
    val strs: Array[String] = new Array(2)
    strs(0) = "hi"
    println(take(strs))

    // Argument position: the element comes from the parameter type.
    println(take(new Array(1)))

    // Assignment, and an array element whose own element is nested.
    var grid: Array[Array[AnyRef]] = null
    grid = new Array(2)
    grid(0) = new Array(1)
    grid(0)(0) = "x"
    println("grid " + grid.length + " " + grid(0)(0))

    // Constructor argument position -- the shape `TrieMap.scala` writes as
    // `new CNode[K, V](0, new Array(0), gen)`. The element is `Cell`'s own
    // type parameter, instantiated at this call site.
    val c = new Cell[String](new Array(2), "c")
    c.slots(1) = "z"
    println(c.slots(1) + " " + c.slots.length + " " + c.tag)

    println(ints.getClass.getName)
    println(strs.getClass.getName)
    println(grid.getClass.getName)
    println(c.slots.getClass.getName)
  }
}
