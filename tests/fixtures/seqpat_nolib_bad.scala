// The private runtime (`--no-scala-library`) has no
// `scala.collection.SeqFactory$UnapplySeqWrapper`, so a sequence pattern on
// `Array` (or `Seq` / `Vector`) has to be a diagnostic, not silently wrong code.
object Main {
  def arr(v: Array[Int]): Int = v match {
    case Array(a, b) => a + b
    case _ => -1
  }
  def main(args: Array[String]): Unit = {
    val v = new Array[Int](2)
    v(0) = 1
    v(1) = 2
    println(arr(v))
  }
}
