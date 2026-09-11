// Arrays of primitives: default values, update, clone independence,
// multi-dimensional arrays, fill/tabulate, sorting, and array operations.
object Main {
  def main(args: Array[String]): Unit = {
    val ia = new Array[Int](3); val da = new Array[Double](2); val ba = new Array[Boolean](2); val ca = new Array[Char](2)
    val la = new Array[Long](1); val sa = new Array[String](2); val fa = new Array[Float](1); val bya = new Array[Byte](1); val sha = new Array[Short](1)
    println(ia.mkString(",") + " " + da.mkString(",") + " " + ba.mkString(",") + " " + ca.map(_.toInt).mkString(",") + " " + la(0) + " " + sa.mkString(",") + " " + fa(0) + " " + bya(0) + " " + sha(0))
    ia(0) = 5; ia(1) += 3; ia(2) = ia(0) * ia(1)
    println(ia.toList)
    val cl = ia.clone(); cl(0) = 99
    println(ia(0) + " " + cl(0))
    val m = Array.ofDim[Int](2, 3)
    for (i <- 0 until 2; j <- 0 until 3) m(i)(j) = i * 10 + j
    println(m.map(_.mkString(" ")).mkString(" | "))
    val jag = Array(Array(1), Array(2, 3), Array[Int]())
    println(jag.map(_.length).toList)
    println(Array.fill(3)(7).toList + " " + Array.tabulate(4)(i => i * i).toList + " " + Array.range(0, 10, 3).toList)
    val unsorted = Array(5, 3, 9, 1)
    scala.util.Sorting.quickSort(unsorted)
    println(unsorted.toList + " " + Array(3.5, -1.0, 2.0).sorted.toList + " " + Array("b", "a").sorted.toList)
    val chars = "hello".toCharArray
    chars(0) = 'j'
    println(new String(chars) + " " + chars.length + " " + chars.reverse.mkString)
    println(Array(1, 2, 3).map(_ * 2).sum + " " + Array(1, 2, 3).filter(_ > 1).length + " " + Array(1, 2, 3).foldLeft(10)(_ - _))
    println(Array(1, 2) ++ Array(3) mkString ",")
    println((Array(1, 2, 3) :+ 4).toList + " " + (0 +: Array(1)).toList)
    println(Array(1, 2, 3).reverse.toList + " " + Array(3, 1, 2).max + " " + Array(1.5, 2.5).min)
    println(java.util.Arrays.equals(Array(1, 2), Array(1, 2)))
    val bytes = "AB".getBytes("UTF-8")
    println(bytes.toList + " " + bytes.map(_ + 1).toList)
    val longs = Array(1L, 2L); longs(1) = longs(0) + Int.MaxValue
    println(longs.toList)
    val bools = Array(true, false); bools(1) = !bools(0)
    println(bools.toList)
    val arrOfArr: Array[Array[Double]] = Array.fill(2, 2)(0.5)
    arrOfArr(1)(1) = 9.0
    println(arrOfArr.map(_.sum).toList)
    println(Array(3, 1, 2).sortWith(_ > _).toList + " " + Array(1, 2, 3, 4).grouped(2).map(_.sum).toList + " " + Array(1, 2, 3).zipWithIndex.toList)
    println(Array.copyOf(Array(1, 2, 3), 5).toList + " " + Array.emptyIntArray.length + " " + Array.empty[Double].length)
    val copyTarget = new Array[Int](5)
    System.arraycopy(Array(9, 8, 7), 0, copyTarget, 1, 3)
    println(copyTarget.toList)
    println(Array(1, 2, 3).slice(1, 3).toList + " " + Array(1, 2, 3).take(2).toList + " " + Array(1, 2, 3).exists(_ == 3) + " " + Array(1, 2, 3).contains(4))
    var total = 0
    for (x <- Array(1, 2, 3)) total += x
    println(total)
  }
}
