// Range コンパニオンの apply / inclusive / count（agent/durrange の 2 件目）。
// javap で確かめたとおり Range$ にあるのは Int 版だけ（BigInt / Long /
// BigDecimal 版は入れ子オブジェクト Range.Long などの側にある）。
// 実 scala-library の jar が要る（--scala-library 専用）。
object Main {
  def main(args: Array[String]): Unit = {
    println(Range(0, 5).toList)
    println(Range(0, 10, 2).toList)
    println(Range.inclusive(1, 3).toList)
    println(Range.inclusive(1, 9, 3).toList)
    println(Range(5, 0, -2).toList)
    println(Range(0, 0).toList)
    println(Range.count(0, 10, 2, false).toString + " " + Range.count(0, 10, 2).toString)
    // 既に動いていた形も一緒に回帰させる。
    println((1 until 10 by 3).toList)
    println((10 to 1 by -2).toList)
    // 型として使ったときも同じ Range であること。
    val r: Range = Range(2, 6)
    println(r.length.toString + " " + r.mkString(","))
  }
}
