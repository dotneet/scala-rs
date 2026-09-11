// Unboxing a `null` that reached a primitive through erased generic code:
// nsc's `BoxesRunTime.unboxToInt(null)` is `0`, not a NullPointerException.
object Main {
  class Cell[T] { var v: T = _ }
  def first[T](xs: Array[AnyRef]): T = xs(0).asInstanceOf[T]
  def main(args: Array[String]): Unit = {
    println(null.asInstanceOf[Int] + null.asInstanceOf[Long] + " " + null.asInstanceOf[Char].toInt)
    val c = new Cell[Int]
    println(c.v + 1)
    val cd = new Cell[Double]
    println(cd.v * 2)
    val o: Option[Int] = Some(null.asInstanceOf[Int])
    println(o.get + 1)
    println(first[Int](Array[AnyRef](null)) + 1)
    val m = new java.util.HashMap[String, Int]()
    println(m.get("missing") + 1)
  }
}
