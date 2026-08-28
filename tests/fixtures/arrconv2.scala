object Main {
  def main(args: Array[String]): Unit = {
    val xs = Array(1, 1, 2, 2, 2, 3)
    println(xs.sum)
    println(xs.product)
    println(xs.min)
    println(xs.max)
    println(xs.minBy(x => -x))
    println(xs.maxBy(x => -x))
    println(xs.reduce((a, b) => a + b))
    println(xs.reduceLeft((a, b) => a + b))
    println(xs.indexWhere(_ == 2))
    println(xs.indexWhere(_ == 2, 3))
    println(xs.lastIndexOf(2, 5))
    println(xs.updated(0, 9).toList)
    println(xs.appended(9).toList)
    println(xs.prepended(9).toList)
    println(xs.concat(List(9, 8)).toList)
    println((xs ++ List(9, 8)).toList)
    println(xs.patch(1, List(9, 8), 2).toList)
    println(xs.zipAll(List(1, 2), 0, 0).toList)
  }
}
