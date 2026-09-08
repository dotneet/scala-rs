// The `ClassTag` half of `arrayelem.scala`: `new Array(n)` whose expected type
// is a type parameter the *call site* instantiates. Library mode only -- the
// private runtime has no `scala.reflect.ClassTag`.
import scala.reflect.ClassTag

object Main {
  def mk[T: ClassTag](n: Int, fill: T): Array[T] = {
    val a: Array[T] = new Array(n)
    var i = 0
    while (i < n) { a(i) = fill; i += 1 }
    a
  }

  def main(args: Array[String]): Unit = {
    val ds = mk[String](2, "d")
    println(ds(0) + ds(1) + " " + ds.length)
    val ns = mk[Int](3, 4)
    println(ns(0) + ns(1) + ns(2))
    println(ds.getClass.getName)
    println(ns.getClass.getName)
  }
}
