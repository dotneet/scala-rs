// `--no-scala-library` has neither `ClassTag` nor `ScalaRunTime.array_update`, so
// building and filling an `Array[T]` at an abstract element type must be a
// **diagnostic**. Hand it a real jar and the same file passes (`arraygen.rs` pins both).
import scala.reflect.ClassTag
object Main {
  def repeat[T: ClassTag](x: T, n: Int): Array[T] = {
    val a = new Array[T](n)
    var i = 0
    while (i < n) { a(i) = x; i += 1 }
    a
  }
  def main(args: Array[String]): Unit = println(repeat(1, 2).length)
}
