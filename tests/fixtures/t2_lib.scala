// slick's `import seq.integral._` (MySQLProfile / JdbcStatementBuilderComponent).
//
// `Numeric[T]` and `Ordering[T]` declare their operators on classes nested
// inside themselves -- `class NumericOps(lhs: T)`, `class OrderingOps(lhs: T)`
// -- and reach them through implicit conversions that are instance members:
//
//     trait Ordering[T] { implicit def mkOrderingOps(lhs: T): OrderingOps }
//     trait Numeric[T] extends Ordering[T] {
//       implicit def mkNumericOps(lhs: T): NumericOps
//     }
//     trait Integral[T] extends Numeric[T] {
//       override implicit def mkNumericOps(lhs: T): IntegralOps
//     }
//
// So `x < zero` needs all of: the conversion to be *in scope* at all (a jar
// class's members are read one name at a time, and an implicit is never named);
// to be read at `integral`'s type argument rather than at `Numeric`'s own `T`;
// the `Integral` override to count as one candidate and not two; and
// `OrderingOps#<(rhs: T)` -- written at `Ordering`'s parameter, not at a
// parameter of `OrderingOps`, which has none -- to be readable and substituted.
object Main {
  def describe[T](integral: Integral[T], x: T): String = {
    import integral._
    val negative = x < zero
    val negated = -x
    val lessOne = x - one
    "" + negative + " " + negated + " " + lessOne
  }

  def biggest[T](ord: Ordering[T], a: T, b: T): T = {
    import ord._
    if (a < b) b else a
  }

  // The same cause reached from the other side: `Option.option2Iterable` is
  // an implicit member of a *companion*, and nothing ever names it either --
  // slick's `where.reduceLeft(f)` and `c.where.toSeq ++ on` on an
  // `Option[Node]` (`JdbcStatementBuilderComponent`).
  def fold(where: Option[String], on: Seq[String]): String =
    where.reduceLeft((a, b) => a + b) + " " + (where.toSeq ++ on).mkString(",")

  def main(args: Array[String]): Unit = {
    println(describe(implicitly[Integral[Int]], 5))
    println(describe(implicitly[Integral[Long]], -7L))
    println(biggest(implicitly[Ordering[String]], "pear", "apple"))
    println(fold(Some("w"), Seq("x", "y")))
  }
}
