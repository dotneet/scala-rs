// Unboxing a `null` that reached a primitive through erased generic code is
// the primitive's zero (nsc's `BoxesRunTime.unboxToInt(null)`), not a
// NullPointerException; a value of the wrong box still fails with a cast.
object Main {
  class Cell[T] { var v: T = _ }
  def first[T](xs: Array[AnyRef]): T = xs(0).asInstanceOf[T]
  def id[T](t: T): T = t
  def main(args: Array[String]): Unit = {
    println(null.asInstanceOf[Int] + null.asInstanceOf[Long] + " " + null.asInstanceOf[Char].toInt)
    val c = new Cell[Int]
    println(c.v + 1)
    val cd = new Cell[Double]
    println(cd.v * 2)
    val cb = new Cell[Boolean]
    println(!cb.v)
    val o: Option[Int] = Some(null.asInstanceOf[Int])
    println(o.get + 1)
    println(first[Int](Array[AnyRef](null)) + 1)
    val m = new java.util.HashMap[String, Int]()
    println(m.get("missing") + 1)
    println(id[Int](3) + id(4L))
    try println(first[Int](Array[AnyRef]("s")) + 1) catch { case _: ClassCastException => println("CCE") }
  }
}
