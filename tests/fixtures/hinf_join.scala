// The join of two sibling library classes under no expected type
// (run/t4658): `if (c) NumericRange.inclusive(…) else NumericRange(…)` is a
// `NumericRange[Int]` -- `NumericRange.Inclusive[Int]` and
// `NumericRange.Exclusive[Int]` share that parent, which the lub finds only
// once the pickle has attached the parents of both jar-loaded classes. A
// `match` joins the same way, and so does a body whose parent is two
// levels up.
import scala.collection.immutable.NumericRange
object Main {
  case class R(start: Int, end: Int, step: Int = 1, inclusive: Boolean = true)
  val rangeData = Array(
    R(1, 10), R(1, 10, 2), R(1, 10, 11), R(-10, -5), R(-10, 0, 2), R(-10, 10, 2),
    R(-10, -5, inclusive = false), R(-10, 0, 2, inclusive = false), R(-10, 10, 2, inclusive = false)
  )
  def ranges = rangeData.map(r => if (r.inclusive) r.start to r.end by r.step else r.start until r.end by r.step)
  def numericIntRanges = rangeData.map(r => if (r.inclusive) NumericRange.inclusive(r.start, r.end, r.step) else NumericRange(r.start, r.end, r.step))
  def numericLongRanges = rangeData.map(r => if (r.inclusive) NumericRange.inclusive(r.start.toLong, r.end, r.step) else NumericRange(r.start.toLong, r.end, r.step))
  def numericBigIntRanges = rangeData.map(r => if (r.inclusive) NumericRange.inclusive(BigInt(r.start), BigInt(r.end), BigInt(r.step)) else NumericRange(BigInt(r.start), BigInt(r.end), BigInt(r.step)))
  def matched = rangeData.map(r => r.inclusive match {
    case true => NumericRange.inclusive(r.start, r.end, r.step)
    case false => NumericRange(r.start, r.end, r.step)
  })
  // `Some` and `None` join at `Option`, two levels up from `Some`'s
  // parents once they are attached.
  def opts = rangeData.map(r => if (r.inclusive) Some(r.start) else None)
  def main(args: Array[String]): Unit = {
    ranges.foreach { range => println(range.sum) }
    numericIntRanges.foreach { range => println(range.sum) }
    numericLongRanges.foreach { range => println(range.sum) }
    numericBigIntRanges.foreach { range => println(range.sum) }
    matched.foreach { range => println(range.sum) }
    println((numericLongRanges zip numericBigIntRanges).forall { case (lr, bir) => lr.sum == bir.sum })
    println((numericIntRanges zip ranges).forall { case (ir, r) => ir.sum == r.sum })
    println(opts.map(_.getOrElse(0)).sum)
  }
}
