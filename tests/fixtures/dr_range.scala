// The Range companion's apply / inclusive / count (the second agent/durrange case).
// As javap confirms, Range$ carries only the Int versions (the BigInt / Long /
// BigDecimal ones live on the nested objects Range.Long and friends).
// Needs the real scala-library jar (--scala-library only).
object Main {
  def main(args: Array[String]): Unit = {
    println(Range(0, 5).toList)
    println(Range(0, 10, 2).toList)
    println(Range.inclusive(1, 3).toList)
    println(Range.inclusive(1, 9, 3).toList)
    println(Range(5, 0, -2).toList)
    println(Range(0, 0).toList)
    println(Range.count(0, 10, 2, false).toString + " " + Range.count(0, 10, 2).toString)
    // Regress the shapes that already worked, alongside.
    println((1 until 10 by 3).toList)
    println((10 to 1 by -2).toList)
    // The same Range when used as a type.
    val r: Range = Range(2, 6)
    println(r.length.toString + " " + r.mkString(","))
  }
}
